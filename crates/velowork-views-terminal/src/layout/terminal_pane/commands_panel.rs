//! `CommandsPanel` — the standalone "Commands" bottom panel.
//!
//! This used to live as a *tab* inside the terminal pane's combined
//! `BottomPanel` (next to the SFTP tab). It has been peeled out of the
//! terminal-pane hierarchy so it can live at the `ProjectColumn` level.
//!
//! Why a separate entity instead of a tab on the SFTP panel:
//!
//! * The commands feature is fundamentally **project/window-scoped**, not
//!   terminal-pane-scoped. It writes a bash command and broadcasts it to a
//!   set of sessions resolved from the shared `TerminalsRegistry` (all online
//!   sessions, the currently focused terminal, or an explicit selection). It
//!   never needs the *specific* `TerminalPane` it used to be nested in.
//! * SFTP, by contrast, is genuinely tied to a single terminal's SSH
//!   session, so it stays in the `TerminalPane`. Splitting the two removes
//!   the artificial coupling where closing/resetting a terminal pane tore
//!   down the commands panel.
//!
//! Communication contract (low coupling):
//!
//! * **Terminal → Commands**: none. The panel never reaches into a
//!   `TerminalPane`. It learns about terminals exclusively through the shared
//!   `TerminalsRegistry` (an `Arc<Mutex<…>>` owned by the app) and the
//!   `FocusManager`/`Workspace` (to resolve "the currently focused
//!   terminal").
//! * **Commands → Terminal**: one-way, via `TerminalsRegistry` —
//!   `Terminal::send_bytes(...)` is the only thing written back.
//! * State changes inside the panel are surfaced to the parent `ProjectColumn`
//!   through GPUI's normal `cx.notify()` + `cx.observe(entity, …)`
//!   mechanism, so the column re-renders when the panel visibility flips.

use velowork_core::theme::ThemeColors;
use velowork_i18n::i18n;
use velowork_terminal::TerminalsRegistry;
use velowork_terminal::shell_config::ShellType;
use velowork_ui::dock::types::AccentColor;
use velowork_ui::dock::{Panel, PanelInfo, PanelKind, ToolbarItem};
use velowork_ui::ControlSize;
use velowork_ui::select::{Select, SelectEvent, SelectOption, SelectPlacement, SelectState, SelectWidthMode};
use velowork_ui::icon::AppIcon;
use velowork_ui::icon_button::icon_button;
use velowork_ui::input::{InputEvent, InputState};
use velowork_ui::overlay_registry::OverlayRegistry;
use velowork_ui::radio::{RadioGroup, RadioMode, RadioOption};
use velowork_ui::simple_input::{InputFocusedEvent, SimpleInput, SimpleInputState};
use velowork_ui::theme::{surface_bg, theme};
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::tokens::{
    ui_text_md, ui_icon_std_ts, SPACE_XS, SPACE_SM, SPACE_MD, SPACE_XL, RADIUS_MD, RADIUS_STD, ICON_SM,
};
use velowork_workspace::focus::FocusManager;
use velowork_workspace::state::{LayoutNode, Workspace};
use velowork_workspace::stores::GlobalSessionStore;

use crate::layout::session_labels::{duplicate_session_suffixes, terminal_base_name};

use crate::elements::resize_handle::ResizeHandle;

use gpui::prelude::*;
use gpui::*;
use std::collections::HashMap;
use std::sync::Arc;
use velowork_ui::h_flex;
use velowork_ui::scrollable::ScrollableElement;
use velowork_ui::tooltip::Tooltip;

/// Execution lifecycle for a (possibly repeating) command broadcast.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandExecutionState {
    Idle,
    Running,
    Paused,
}

/// Shared mutable flags toggled by the run loop and the pause/stop controls.
pub struct CommandRunState {
    pub paused: bool,
    pub stopped: bool,
}

/// Tracks an in-progress bottom-panel resize drag for the commands panel.
#[derive(Clone, Copy, Debug)]
struct ResizeDragState {
    start_y: f32,
    start_height: f32,
}

pub struct CommandsPanel {
    // Shared terminal registry: the only channel back to live terminals.
    terminals: TerminalsRegistry,
    workspace: Entity<Workspace>,
    focus_manager: Entity<FocusManager>,

    // Command input + broadcast options
    command_input: Entity<SimpleInputState>,
    repeat_input: Option<Entity<InputState>>,
    interval_input: Option<Entity<InputState>>,
    repeat_val: String,
    interval_val: String,
    selected_sessions: std::collections::HashSet<String>,
    target_mode: String,
    target_select: Entity<SelectState<String>>,

    // Send mode: "line" (each line separately) or "block" (all at once)
    send_mode: String,

    // Run lifecycle
    cmd_state: CommandExecutionState,
    run_state: Option<Arc<parking_lot::Mutex<CommandRunState>>>,

    // Visibility / layout
    is_visible: bool,
    is_collapsed: bool,
    is_maximized: bool,
    /// Height to restore to when leaving the maximized state.
    restore_height: f32,
    panel_height: f32,
    _height_anim_task: Option<Task<()>>,
    resize_dragging: Option<ResizeDragState>,
    commands_focus_pending: bool,

    focus_handle: FocusHandle,

    /// Registry for centralized click-outside dismissal of the target-host
    /// dropdown. `None` keeps the legacy local-backdrop behavior.
    overlay_registry: Option<Entity<OverlayRegistry>>,

    /// Weak self-reference for toolbar callbacks that need entity access.
    self_weak: WeakEntity<Self>,
}

impl CommandsPanel {
    pub fn new(
        terminals: TerminalsRegistry,
        workspace: Entity<Workspace>,
        focus_manager: Entity<FocusManager>,
        overlay_registry: Option<Entity<OverlayRegistry>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let command_input = cx.new(|cx| {
            let mut state = SimpleInputState::new(cx)
                .placeholder("")
                .multiline()
                .syntax_language(Some("bash"));
            state.set_fill_height(true);
            state.set_show_gutter(true);
            // Enable soft-wrap so long commands fold to multiple visual rows
            // instead of scrolling off the right edge. The wrap width is derived
            // from the input's live bounds, so it tracks the panel/dock resize.
            state.set_wrap(true);
            state
        });
        // Focusing command input must clear the focused terminal so key
        // strokes go to the command box and not the underlying shell.
        cx.subscribe(&command_input, |this, _, _: &InputFocusedEvent, cx| {
            this.focus_manager.update(cx, |fm, cx| {
                this.workspace.update(cx, |ws, cx| {
                    ws.clear_focused_terminal(fm, cx);
                });
            });
            cx.notify();
        })
        .detach();

        let target_select = cx.new(|cx| {
            SelectState::new(cx)
                .options(vec![
                    SelectOption::new("Current Terminal".to_string(), i18n!(cx, "sftp.commands.target.current")),
                    SelectOption::new("Selected Sessions".to_string(), i18n!(cx, "sftp.commands.target.selected")),
                    SelectOption::new("All Online".to_string(), i18n!(cx, "sftp.commands.target.all")),
                ])
                .selected(Some("Current Terminal".to_string()))
                .placement(SelectPlacement::Below)
                .width_mode(SelectWidthMode::ContentAdaptive)
                .size(ControlSize::Compact)
        });

        cx.subscribe(&target_select, |this, _, event: &SelectEvent<String>, cx| {
            if let SelectEvent::Change(Some(mode)) = event {
                this.target_mode = mode.clone();
                cx.notify();
            }
        })
        .detach();

        Self {
            terminals,
            workspace,
            focus_manager,
            command_input,
            repeat_input: None,
            interval_input: None,
            repeat_val: "1".to_string(),
            interval_val: "1".to_string(),
            selected_sessions: std::collections::HashSet::new(),
            target_mode: "Current Terminal".to_string(),
            target_select,
            send_mode: "block".to_string(),
            cmd_state: CommandExecutionState::Idle,
            run_state: None,
            is_visible: false,
            is_collapsed: false,
            is_maximized: false,
            restore_height: 180.0,
            panel_height: 180.0,
            _height_anim_task: None,
            resize_dragging: None,
            commands_focus_pending: false,
            focus_handle: cx.focus_handle(),
            overlay_registry,
            self_weak: cx.entity().downgrade(),
        }
    }

    pub fn ensure_toolbar_inputs(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.repeat_input.is_none() {
            let val = self.repeat_val.clone();
            let repeat_input = cx.new(|cx| {
                InputState::new(cx).default_value(val)
            });
            cx.subscribe(&repeat_input, |this: &mut Self, _, event: &InputEvent, cx| {
                if let InputEvent::Change = event {
                    this.repeat_val = this
                        .repeat_input
                        .as_ref()
                        .map(|input| input.read(cx).text().to_string())
                        .unwrap_or_default();
                }
            })
            .detach();
            self.repeat_input = Some(repeat_input);
        }

        if self.interval_input.is_none() {
            let val = self.interval_val.clone();
            let interval_input = cx.new(|cx| {
                InputState::new(cx).default_value(val)
            });
            cx.subscribe(&interval_input, |this: &mut Self, _, event: &InputEvent, cx| {
                if let InputEvent::Change = event {
                    this.interval_val = this
                        .interval_input
                        .as_ref()
                        .map(|input| input.read(cx).text().to_string())
                        .unwrap_or_default();
                }
            })
            .detach();
            self.interval_input = Some(interval_input);
        }
    }

    pub fn set_command_text(&mut self, cmd: &str, cx: &mut Context<Self>) {
        self.command_input.update(cx, |input, cx| {
            input.set_value(cmd, cx);
        });
        cx.notify();
    }

    pub fn animate_height_to(&mut self, target_height: f32, cx: &mut Context<Self>) {
        let start_height = self.panel_height;
        if (target_height - start_height).abs() < 1.0 {
            self.panel_height = target_height;
            cx.notify();
            return;
        }

        let steps = 9;
        let step_dur = std::time::Duration::from_millis(160) / steps;
        self._height_anim_task = Some(cx.spawn(async move |this: WeakEntity<Self>, cx| {
            for i in 1..=steps {
                smol::Timer::after(step_dur).await;
                let t = i as f32 / steps as f32;
                let ease_t = velowork_ui::motion::ease_out_cubic(t);
                let current = start_height + (target_height - start_height) * ease_t;
                let res = this.update(cx, |this, cx| {
                    this.panel_height = current;
                    cx.notify();
                });
                if res.is_err() {
                    break;
                }
            }
            let _ = this.update(cx, |this, cx| {
                this.panel_height = target_height;
                this._height_anim_task = None;
                cx.notify();
            });
        }));
    }

    /// Toggle the whole panel's visibility. Driven by the status-bar icon and
    /// the `ToggleCommandsPanel` hotkey.
    ///
    /// The toggle operates on *effective* visibility, not the raw `is_visible`
    /// flag. A collapsed panel (`is_collapsed == true`) is rendered with a
    /// zero-height body and no on-panel control to re-expand it, so it reads as
    /// "not shown" to the user even though `is_visible` is still `true`
    /// (collapse is a separate on-panel action from hide). If we toggled the
    /// raw flag, a collapsed panel would take *two* status-bar clicks to
    /// re-surface (first click flips `is_visible` false → still hidden; second
    /// click flips it true but `is_collapsed` stays true → still hidden), which
    /// is exactly the "icon click does nothing" bug. By treating
    /// `!is_visible || is_collapsed` as the hidden state, a single click after
    /// a collapse immediately re-expands the panel, and a single click on a
    /// fully-shown panel hides it.
    pub fn toggle(&mut self, cx: &mut Context<Self>) {
        let effectively_hidden = !self.is_visible || self.is_collapsed;
        self.is_visible = effectively_hidden;
        if self.is_visible {
            self.is_collapsed = false;
            self.commands_focus_pending = true;
        }
        cx.notify();
    }

    /// Open and expand the panel with a prefilled command string.
    pub fn open_with_command(&mut self, cmd: &str, cx: &mut Context<Self>) {
        self.is_visible = true;
        self.is_collapsed = false;
        self.commands_focus_pending = true;
        self.command_input.update(cx, |input, cx| {
            input.set_value(cmd, cx);
        });
        cx.notify();
    }

    /// Maximize the panel to a preset "full" size, or restore the previous
    /// height when already maximized. Mirrors the window maximize/restore
    /// affordance but scoped to the bottom panel: the preset size fills most
    /// of the window viewport, leaving room for the title bar, project header
    /// and status bar chrome.
    pub fn toggle_maximized(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.is_maximized {
            let target = if self.restore_height > 0.0 {
                self.restore_height
            } else {
                180.0
            };
            self.is_maximized = false;
            self.animate_height_to(target, cx);
        } else {
            self.restore_height = self.panel_height;
            let chrome = px(110.0);
            let max_h = f32::from(window.viewport_size().height) - f32::from(chrome);
            let target = max_h.max(240.0);
            self.is_maximized = true;
            self.animate_height_to(target, cx);
        }
    }

    pub fn is_visible(&self) -> bool {
        self.is_visible
    }

    pub fn toggle_collapsed(&mut self, cx: &mut Context<Self>) {
        self.is_collapsed = !self.is_collapsed;
        cx.notify();
    }

    fn pause_commands(&mut self, cx: &mut Context<Self>) {
        if let Some(ref state) = self.run_state {
            state.lock().paused = true;
            self.cmd_state = CommandExecutionState::Paused;
            cx.notify();
        }
    }

    fn resume_commands(&mut self, cx: &mut Context<Self>) {
        if let Some(ref state) = self.run_state {
            state.lock().paused = false;
            self.cmd_state = CommandExecutionState::Running;
            cx.notify();
        }
    }

    fn stop_commands(&mut self, cx: &mut Context<Self>) {
        if let Some(ref state) = self.run_state {
            state.lock().stopped = true;
        }
        self.run_state = None;
        self.cmd_state = CommandExecutionState::Idle;
        cx.notify();
    }

    /// Inject the window-level `OverlayRegistry` so the target-host dropdown
    /// participates in centralized click-outside dismissal.
    pub fn set_overlay_registry(&mut self, reg: Entity<OverlayRegistry>) {
        self.overlay_registry = Some(reg);
    }



    fn run_commands(&mut self, cx: &mut Context<Self>) {
        if self.cmd_state == CommandExecutionState::Paused {
            self.resume_commands(cx);
            return;
        }

        if self.cmd_state == CommandExecutionState::Running {
            return;
        }

        let cmd = self.command_input.read(cx).value().trim().to_string();
        if cmd.is_empty() {
            return;
        }

        // Get target terminals
        let mut target_ids = Vec::new();
        match self.target_mode.as_str() {
            "Current Terminal" => {
                let focused_state = self.focus_manager.read(cx).last_focused_terminal_state();
                if let Some(focused) = focused_state {
                    if let Some(project) = self.workspace.read(cx).project(&focused.project_id) {
                        if let Some(layout) = project.layout.as_ref() {
                            if let Some(velowork_workspace::state::LayoutNode::Terminal {
                                terminal_id: Some(id),
                                ..
                            }) = layout.get_at_path(&focused.layout_path)
                            {
                                target_ids.push(id.clone());
                            }
                        }
                    }
                }
                if target_ids.is_empty() {
                    if let Some(proj_id) = self.focus_manager.read(cx).active_project_id() {
                        if let Some(proj) = self.workspace.read(cx).project(proj_id) {
                            if let Some(ref layout) = proj.layout {
                                let mut ordered = Vec::new();
                                collect_shell_types_ordered(layout, proj_id, &mut ordered);
                                if let Some((first_tid, _, _)) = ordered.first() {
                                    target_ids.push(first_tid.clone());
                                }
                            }
                        }
                    }
                }
            }
            "Selected Sessions" => {
                target_ids = self.selected_sessions.iter().cloned().collect();
            }
            "All Online" | _ => {
                let terminals = self.terminals.lock();
                target_ids = terminals.keys().cloned().collect();
            }
        }

        if target_ids.is_empty() {
            return;
        }

        // Parse repeat and interval
        let repeat_count = self
            .repeat_input
            .as_ref()
            .map(|i| i.read(cx).text().to_string())
            .unwrap_or_else(|| self.repeat_val.clone())
            .parse::<usize>()
            .unwrap_or(1);
        let interval_str = self
            .interval_input
            .as_ref()
            .map(|i| i.read(cx).text().to_string())
            .unwrap_or_else(|| self.interval_val.clone());
        let interval_ms = if interval_str.ends_with('s') {
            interval_str[..interval_str.len() - 1]
                .parse::<f64>()
                .unwrap_or(1.0)
                * 1000.0
        } else {
            interval_str.parse::<f64>().unwrap_or(1.0) * 1000.0
        } as u64;

        let run_state = Arc::new(parking_lot::Mutex::new(CommandRunState {
            paused: false,
            stopped: false,
        }));
        self.run_state = Some(run_state.clone());
        self.cmd_state = CommandExecutionState::Running;

        // Record the whole multi-line batch command as a single history entry
        let cmd_trimmed = cmd.trim().to_string();
        if !cmd_trimmed.is_empty() {
            if let Some(db) = velowork_core::storage::database() {
                let repo = velowork_workspace::repositories::HistoryRepository::new(db);
                let s = crate::terminal_view_settings(cx);
                let active_pid = self
                    .focus_manager
                    .read(cx)
                    .active_project_id()
                    .map(|id| id.to_string())
                    .or_else(|| self.workspace.read(cx).projects().first().map(|p| p.id.clone()));
                if let Some(pid) = active_pid {
                    let _ = repo.record_project_command(
                        &pid,
                        &cmd_trimmed,
                        s.command_history_max_count,
                        s.command_history_retention_days,
                    );
                }
            }
        }

        cx.notify();

        let terminals_registry = self.terminals.clone();
        let send_mode = self.send_mode.clone();

        cx.spawn(async move |this, cx| {
            for i in 0..repeat_count {
                if i > 0 {
                    let interval = std::time::Duration::from_millis(interval_ms);
                    let step = std::time::Duration::from_millis(100);
                    let mut elapsed = std::time::Duration::ZERO;

                    while elapsed < interval {
                        let (paused, stopped) = {
                            let state = run_state.lock();
                            (state.paused, state.stopped)
                        };
                        if stopped {
                            let _ = this.update(cx, |this, cx| {
                                this.cmd_state = CommandExecutionState::Idle;
                                this.run_state = None;
                                cx.notify();
                            });
                            return;
                        }
                        if paused {
                            smol::Timer::after(step).await;
                            continue;
                        }

                        smol::Timer::after(step).await;
                        elapsed += step;
                    }
                }

                // Check paused or stopped state before iteration
                loop {
                    let (paused, stopped) = {
                        let state = run_state.lock();
                        (state.paused, state.stopped)
                    };
                    if stopped {
                        let _ = this.update(cx, |this, cx| {
                            this.cmd_state = CommandExecutionState::Idle;
                            this.run_state = None;
                            cx.notify();
                        });
                        return;
                    }
                    if !paused {
                        break;
                    }
                    smol::Timer::after(std::time::Duration::from_millis(100)).await;
                }

                // Snapshot active terminals without holding lock during send or timer awaits
                let active_terminals: Vec<std::sync::Arc<velowork_terminal::terminal::Terminal>> = {
                    let terminals = terminals_registry.lock();
                    target_ids
                        .iter()
                        .filter_map(|id| terminals.get(id).cloned())
                        .collect()
                };

                if active_terminals.is_empty() {
                    break;
                }

                if send_mode == "line" {
                    // Line-by-line mode: execute each line with delay
                    for line in cmd.lines() {
                        let (paused, stopped) = {
                            let state = run_state.lock();
                            (state.paused, state.stopped)
                        };
                        if stopped {
                            let _ = this.update(cx, |this, cx| {
                                this.cmd_state = CommandExecutionState::Idle;
                                this.run_state = None;
                                cx.notify();
                            });
                            return;
                        }
                        if paused {
                            loop {
                                smol::Timer::after(std::time::Duration::from_millis(100)).await;
                                let (p, s) = {
                                    let state = run_state.lock();
                                    (state.paused, state.stopped)
                                };
                                if s {
                                    let _ = this.update(cx, |this, cx| {
                                        this.cmd_state = CommandExecutionState::Idle;
                                        this.run_state = None;
                                        cx.notify();
                                    });
                                    return;
                                }
                                if !p {
                                    break;
                                }
                            }
                        }

                        let trimmed = line.trim_end_matches(['\r', '\n']);
                        if !trimmed.is_empty() {
                            let mut line_str = trimmed.to_string();
                            line_str.push('\r');
                            for term in &active_terminals {
                                term.send_bytes(line_str.as_bytes());
                            }
                            smol::Timer::after(std::time::Duration::from_millis(50)).await;
                        }
                    }
                } else {
                    // Block mode: format multi-line commands with terminal backslash line continuations
                    let formatted_block = format_block_command(&cmd);
                    if !formatted_block.is_empty() {
                        for term in &active_terminals {
                            term.send_bytes(formatted_block.as_bytes());
                        }
                    }
                }
            }

            let _ = this.update(cx, |this, cx| {
                this.cmd_state = CommandExecutionState::Idle;
                this.run_state = None;
                cx.notify();
            });
        })
        .detach();
    }

    pub fn focus_input(&mut self, cx: &mut Context<Self>) {
        self.commands_focus_pending = true;
        cx.notify();
    }

    pub fn toggle_visible_direct(&mut self, visible: bool, cx: &mut Context<Self>) {
        self.is_visible = visible;
        if visible {
            self.is_collapsed = false;
            self.commands_focus_pending = true;
        }
        cx.notify();
    }

    pub fn render_header_controls(&self, t: &ThemeColors, cx: &mut Context<Self>) -> AnyElement {
        let p = SemanticPalette::from_theme(t);
        let play_enabled = self.cmd_state != CommandExecutionState::Running;
        let play_tooltip = i18n!(cx, "sftp.commands.tooltip.run");
        let play_btn =
            div()
                .id("cmd-run-btn")
                .flex_shrink_0()
                .w(px(24.0))
                .h(px(24.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(RADIUS_STD)
                .child(AppIcon::Play.size(ui_icon_std_ts(cx)).text_color(rgb(
                    if play_enabled {
                        t.text_primary
                    } else {
                        t.text_muted
                    },
                )));
        let play_btn = if play_enabled {
            play_btn
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.run_commands(cx);
                }))
        } else {
            play_btn
        };
        let play_btn = play_btn.tooltip(move |_, cx| {
            let __tip = play_tooltip.clone();
            cx.new(|_| Tooltip::new(__tip)).into()
        });

        let pause_enabled = self.cmd_state == CommandExecutionState::Running;
        let pause_tooltip = i18n!(cx, "sftp.commands.tooltip.pause");
        let pause_btn = div()
            .id("cmd-pause-btn")
            .flex_shrink_0()
            .w(px(24.0))
            .h(px(24.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(RADIUS_STD)
            .child(AppIcon::Pause.size(ui_icon_std_ts(cx)).text_color(rgb(
                if pause_enabled {
                    t.text_primary
                } else {
                    t.text_muted
                },
            )));
        let pause_btn = if pause_enabled {
            pause_btn
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.pause_commands(cx);
                }))
        } else {
            pause_btn
        };
        let pause_btn = pause_btn.tooltip(move |_, cx| {
            let __tip = pause_tooltip.clone();
            cx.new(|_| Tooltip::new(__tip)).into()
        });

        let stop_enabled = self.cmd_state != CommandExecutionState::Idle;
        let stop_tooltip = i18n!(cx, "sftp.commands.tooltip.stop");
        let stop_btn =
            div()
                .id("cmd-stop-btn")
                .flex_shrink_0()
                .w(px(24.0))
                .h(px(24.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(RADIUS_STD)
                .child(AppIcon::Stop.size(ui_icon_std_ts(cx)).text_color(rgb(
                    if stop_enabled {
                        t.text_primary
                    } else {
                        t.text_muted
                    },
                )));
        let stop_btn = if stop_enabled {
            stop_btn
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.stop_commands(cx);
                }))
        } else {
            stop_btn
        };
        let stop_btn = stop_btn.tooltip(move |_, cx| {
            let __tip = stop_tooltip.clone();
            cx.new(|_| Tooltip::new(__tip)).into()
        });



        let line_label = i18n!(cx, "sftp.commands.send_line");
        let block_label = i18n!(cx, "sftp.commands.send_block");
        let line_tip = i18n!(cx, "sftp.commands.tooltip.send_line");
        let block_tip = i18n!(cx, "sftp.commands.tooltip.send_block");

        let this_for_radio = cx.entity().downgrade();
        let send_mode_switcher = RadioGroup::new("cmd-header-send-mode-radio")
            .mode(RadioMode::Button)
            .compact()
            .options(vec![
                RadioOption::new("line".to_string(), line_label).tooltip(line_tip),
                RadioOption::new("block".to_string(), block_label).tooltip(block_tip),
            ])
            .selected(Some(self.send_mode.clone()))
            .on_change(move |val: &String, _window, cx| {
                if let Some(entity) = this_for_radio.upgrade() {
                    let val = val.clone();
                    let _ = entity.update(cx, |this, cx| {
                        if this.send_mode != val {
                            this.send_mode = val;
                            cx.notify();
                        }
                    });
                }
            });

        h_flex()
            .gap(SPACE_XS)
            .child(play_btn)
            .child(pause_btn)
            .child(stop_btn)
            .child(div().w(px(1.0)).h(SPACE_XL).bg(p.border_subtle))
            .child(send_mode_switcher)
            .child(div().w(px(1.0)).h(SPACE_XL).bg(p.border_subtle))
            .child(
                div()
                    .flex_shrink_0()
                    .text_size(ui_text_md(cx))
                    .text_color(p.text_secondary)
                    .child(i18n!(cx, "sftp.commands.repeat")),
            )
            .child(
                if let Some(ref repeat_input) = self.repeat_input {
                    SimpleInput::new(repeat_input).w(px(60.0)).into_any_element()
                } else {
                    div().into_any_element()
                },
            )
            .child(
                div()
                    .flex_shrink_0()
                    .text_size(ui_text_md(cx))
                    .text_color(p.text_secondary)
                    .child(i18n!(cx, "sftp.commands.interval")),
            )
            .child(
                if let Some(ref interval_input) = self.interval_input {
                    SimpleInput::new(interval_input).w(px(72.0)).into_any_element()
                } else {
                    div().into_any_element()
                },
            )
            .child(div().w(px(1.0)).h(SPACE_XL).bg(p.border_subtle))
            .child(
                div()
                    .text_size(ui_text_md(cx))
                    .text_color(p.text_secondary)
                    .child(i18n!(cx, "sftp.commands.target.label")),
            )
            .child(Select::new(&self.target_select))
            .into_any_element()
    }

    pub fn render_body_with_dropdown(
        &self,
        t: &ThemeColors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = SemanticPalette::from_theme(t);
        let conn_names = self.session_connection_names(cx);
        let any_focused = self
            .command_input
            .read(cx)
            .focus_handle(cx)
            .is_focused(window)
            || self
                .repeat_input
                .as_ref()
                .map(|i| i.read(cx).focus_handle(cx).is_focused(window))
                .unwrap_or(false)
            || self
                .interval_input
                .as_ref()
                .map(|i| i.read(cx).focus_handle(cx).is_focused(window))
                .unwrap_or(false);

        if any_focused {
            let is_modal = self.focus_manager.read(cx).is_modal();
            if !is_modal {
                self.focus_manager.update(cx, |fm, cx| {
                    self.workspace.update(cx, |ws, cx| {
                        ws.clear_focused_terminal(fm, cx);
                    });
                });
            }
        }

        // Auto-focus the command input once after opening the panel.
        if self.commands_focus_pending {
            let fh = self.command_input.read(cx).focus_handle(cx);
            if !fh.is_focused(window) {
                window.focus(&fh, cx);
            }
            self.focus_manager.update(cx, |fm, cx| {
                self.workspace.update(cx, |ws, cx| {
                    ws.clear_focused_terminal(fm, cx);
                });
            });
            // Defer clearing the flag until after this render pass.
            let entity = cx.entity().downgrade();
            window.on_next_frame(move |_window, cx| {
                if let Some(entity) = entity.upgrade() {
                    entity.update(cx, |this, cx| {
                        this.commands_focus_pending = false;
                        cx.notify();
                    });
                }
            });
        }

        let body = h_flex()
            .size_full()
            .gap(SPACE_XL)
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .flex()
                    .flex_col()
                    .min_h(px(0.0))
                    .overflow_hidden()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _event, window, cx| {
                            this.command_input.update(cx, |input, cx| {
                                input.focus(window, cx);
                            });
                            this.focus_manager.update(cx, |fm, cx| {
                                this.workspace.update(cx, |ws, cx| {
                                    ws.clear_focused_terminal(fm, cx);
                                });
                            });
                        }),
                    )
                    .child(
                        SimpleInput::new(&self.command_input)
                            .text_size(ui_text_md(cx))
                            .fill_height()
                            .appearance(false)
                            .focus_ring(false),
                    ),
            )
            .when(self.target_mode == "Selected Sessions", |el| {
                el.child(
                    div()
                        .w(px(240.0))
                        .h_full()
                        .bg(surface_bg(t.bg_secondary, cx))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .overflow_y_scrollbar()
                                .children({
                                    let session_order = self.session_order(cx);

                                    session_order.into_iter().enumerate().map(|(idx, tid)| {
                                        let is_checked = self.selected_sessions.contains(&tid);
                                        let display = conn_names
                                            .get(&tid)
                                            .cloned()
                                            .unwrap_or_else(|| tid.clone());
                                        let tid_clone = tid.clone();

                                        h_flex()
                                            .id(ElementId::Name(
                                                format!("session-chk-{}", idx).into(),
                                            ))
                                            .px(SPACE_MD)
                                            .py(SPACE_XS)
                                            .gap(SPACE_MD)
                                            .items_center()
                                            .cursor_pointer()
                                            .hover(|s| s.bg(rgb(t.bg_hover)))
                                            .child(
                                                div()
                                                    .w(px(14.0))
                                                    .h(px(14.0))
                                                    .border_1()
                                                    .border_color(p.border_subtle)
                                                    .rounded(RADIUS_MD)
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .bg(rgb(if is_checked {
                                                        t.bg_selection
                                                    } else {
                                                        t.bg_primary
                                                    }))
                                                    .when(is_checked, |el| {
                                                        el.child(
                                                                AppIcon::Check
                                                                .size(ICON_SM)
                                                                .text_color(p.text_primary),
                                                        )
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .text_size(ui_text_md(cx))
                                                    .text_color(p.text_primary)
                                                    .child(display),
                                            )
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if this.selected_sessions.contains(&tid_clone) {
                                                    this.selected_sessions.remove(&tid_clone);
                                                } else {
                                                    this.selected_sessions
                                                        .insert(tid_clone.clone());
                                                }
                                                cx.notify();
                                            }))
                                    })
                                }),
                        ),
                )
            });

        div()
            .size_full()
            .relative()
            .child(body)
            .into_any_element()
    }

    /// Resolve the user-facing connection name for every online terminal, using
    /// the exact same base-name logic as the terminal tab strip (custom terminal
    /// name / OSC title / directory for remote backends, custom name or shell
    /// name for local terminals, saved session name for SSH shells). Used by the
    /// "Selected Sessions" picker so a terminal is labelled identically there
    /// and in the tab bar.
    /// Ordered list of *online* terminal ids, in the same reading order the tab
    /// strip and tab-list dropdown use (project display order, then in-layout
    /// reading order). Drives the "Selected Sessions" host list so its ordering
    /// matches the tabs instead of an unrelated alphabetical sort.
    fn session_order(&self, cx: &Context<Self>) -> Vec<String> {
        let mut ordered: Vec<(String, String, ShellType)> = Vec::new();
        {
            let ws = self.workspace.read(cx);
            for project in ws.projects() {
                if let Some(layout) = &project.layout {
                    collect_shell_types_ordered(layout, &project.id, &mut ordered);
                }
            }
        }
        let online: std::collections::HashSet<String> =
            self.terminals.lock().keys().cloned().collect();
        ordered
            .into_iter()
            .filter(|(tid, _, _)| online.contains(tid))
            .map(|(tid, _, _)| tid)
            .collect()
    }

    fn session_connection_names(&self, cx: &Context<Self>) -> HashMap<String, String> {
        // Walk each project's layout tree in reading order, mapping every
        // terminal to its shell type *and* owning project id so we resolve the
        // same custom name / OSC title / directory as the tab strip. Order here
        // is irrelevant (we key by id); `session_order` reuses the same walk for
        // the *display* order of the host list.
        let mut ordered: Vec<(String, String, ShellType)> = Vec::new();
        {
            let ws = self.workspace.read(cx);
            for project in ws.projects() {
                if let Some(layout) = &project.layout {
                    collect_shell_types_ordered(layout, &project.id, &mut ordered);
                }
            }
        }

        let store = cx.global::<GlobalSessionStore>().0.read(cx);
        let terminals = self.terminals.lock();
        let mut names = HashMap::new();
        for (tid, project_id, shell_type) in &ordered {
            let backend_is_remote = self
                .workspace
                .read(cx)
                .project(project_id)
                .map(|p| p.is_remote)
                .unwrap_or(false);
            let osc_title = terminals.get(tid).and_then(|t| t.title());

            let name = if let Some(project) = self.workspace.read(cx).project(project_id) {
                terminal_base_name(
                    tid,
                    shell_type,
                    backend_is_remote,
                    osc_title.as_deref(),
                    project,
                    &store,
                )
            } else {
                shell_type.local_shell_name()
            };
            names.insert(tid.clone(), name);
        }

        // Apply the single, global duplicate-session numbering so the
        // "Selected Sessions" host list shows the exact same `:N` suffixes as
        // the terminal tabs (e.g. `名称`, `名称:2`, `名称:3`).
        let suffixes = duplicate_session_suffixes(&self.workspace.read(cx));
        for (tid, suffix) in &suffixes {
            if let Some(name) = names.get_mut(tid) {
                *name = format!("{}:{}", name, suffix);
            }
        }

        names
    }

    /// Render the panel. Returns an empty element when hidden so the parent
    /// column can always call it unconditionally (mirrors `ServicePanel`).
    pub fn render_panel(
        &self,
        t: &ThemeColors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if !self.is_visible {
            return div().into_any_element();
        }

        let p = SemanticPalette::from_theme(t);
        let conn_names = self.session_connection_names(cx);
        let any_focused = self
            .command_input
            .read(cx)
            .focus_handle(cx)
            .is_focused(window)
            || self
                .repeat_input
                .as_ref()
                .map(|i| i.read(cx).focus_handle(cx).is_focused(window))
                .unwrap_or(false)
            || self
                .interval_input
                .as_ref()
                .map(|i| i.read(cx).focus_handle(cx).is_focused(window))
                .unwrap_or(false);

        if any_focused {
            let is_modal = self.focus_manager.read(cx).is_modal();
            if !is_modal {
                self.focus_manager.update(cx, |fm, cx| {
                    self.workspace.update(cx, |ws, cx| {
                        ws.clear_focused_terminal(fm, cx);
                    });
                });
            }
        }

        // Auto-focus the command input once after opening the panel.
        if self.commands_focus_pending {
            let fh = self.command_input.read(cx).focus_handle(cx);
            if !fh.is_focused(window) {
                window.focus(&fh, cx);
            }
            self.focus_manager.update(cx, |fm, cx| {
                self.workspace.update(cx, |ws, cx| {
                    ws.clear_focused_terminal(fm, cx);
                });
            });
            // Defer clearing the flag until after this render pass.
            let entity = cx.entity().downgrade();
            window.on_next_frame(move |_window, cx| {
                if let Some(entity) = entity.upgrade() {
                    entity.update(cx, |this, cx| {
                        this.commands_focus_pending = false;
                        cx.notify();
                    });
                }
            });
        }

        if self.is_collapsed {
            return div().into_any_element();
        }

        // ─── Header (always visible while the panel is open) ───
        let play_enabled = self.cmd_state != CommandExecutionState::Running;
        let play_tooltip = i18n!(cx, "sftp.commands.tooltip.run");
        let play_btn =
            div()
                .id("cmd-run-btn")
                .flex_shrink_0()
                .w(px(24.0))
                .h(px(24.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(RADIUS_STD)
                .child(AppIcon::Play.size(ui_icon_std_ts(cx)).text_color(rgb(
                    if play_enabled {
                        t.text_primary
                    } else {
                        t.text_muted
                    },
                )));
        let play_btn = if play_enabled {
            play_btn
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.run_commands(cx);
                }))
        } else {
            play_btn
        };
        let play_btn = play_btn.tooltip(move |_, cx| {
            let __tip = play_tooltip.clone();
            cx.new(|_| Tooltip::new(__tip)).into()
        });

        let pause_enabled = self.cmd_state == CommandExecutionState::Running;
        let pause_tooltip = i18n!(cx, "sftp.commands.tooltip.pause");
        let pause_btn = div()
            .id("cmd-pause-btn")
            .flex_shrink_0()
            .w(px(24.0))
            .h(px(24.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(RADIUS_STD)
            .child(AppIcon::Pause.size(ui_icon_std_ts(cx)).text_color(rgb(
                if pause_enabled {
                    t.text_primary
                } else {
                    t.text_muted
                },
            )));
        let pause_btn = if pause_enabled {
            pause_btn
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.pause_commands(cx);
                }))
        } else {
            pause_btn
        };
        let pause_btn = pause_btn.tooltip(move |_, cx| {
            let __tip = pause_tooltip.clone();
            cx.new(|_| Tooltip::new(__tip)).into()
        });

        let stop_enabled = self.cmd_state != CommandExecutionState::Idle;
        let stop_tooltip = i18n!(cx, "sftp.commands.tooltip.stop");
        let stop_btn =
            div()
                .id("cmd-stop-btn")
                .flex_shrink_0()
                .w(px(24.0))
                .h(px(24.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(RADIUS_STD)
                .child(AppIcon::Stop.size(ui_icon_std_ts(cx)).text_color(rgb(
                    if stop_enabled {
                        t.text_primary
                    } else {
                        t.text_muted
                    },
                )));
        let stop_btn = if stop_enabled {
            stop_btn
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.stop_commands(cx);
                }))
        } else {
            stop_btn
        };
        let stop_btn = stop_btn.tooltip(move |_, cx| {
            let __tip = stop_tooltip.clone();
            cx.new(|_| Tooltip::new(__tip)).into()
        });



        let header =
            h_flex()
                .h(px(32.0))
                .px(SPACE_MD)
                .bg(rgb(t.bg_header))
                .border_b_1()
                .border_color(p.border_subtle)
                .justify_between()
                .child(
                    h_flex()
                        .gap(SPACE_SM)
                        .px(SPACE_MD)
                        .py(SPACE_XS)
                        .child(
                            AppIcon::CommandAction
                                .size(ui_icon_std_ts(cx))
                                .text_color(p.text_primary),
                        )
                        .child(
                            div()
                                .text_size(ui_text_md(cx))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(p.text_primary)
                                .child(i18n!(cx, "sftp.commands.title")),
                        ),
                )
                .child(
                    h_flex()
                        .gap(SPACE_XS)
                        .child(play_btn)
                        .child(pause_btn)
                        .child(stop_btn)
                        .child(div().w(px(1.0)).h(SPACE_XL).bg(p.border_subtle))
                        .child(
                            div()
                                 .flex_shrink_0()
                                 .text_size(ui_text_md(cx))
                                 .text_color(p.text_secondary)
                                 .child(i18n!(cx, "sftp.commands.repeat")),
                        )
                        .child(
                            if let Some(ref repeat_input) = self.repeat_input {
                                SimpleInput::new(repeat_input).w(px(60.0)).into_any_element()
                            } else {
                                div().into_any_element()
                            },
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_size(ui_text_md(cx))
                                .text_color(p.text_secondary)
                                .child(i18n!(cx, "sftp.commands.interval")),
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .child(
                                    if let Some(ref interval_input) = self.interval_input {
                                        SimpleInput::new(interval_input).w(px(72.0)).into_any_element()
                                    } else {
                                        div().into_any_element()
                                    },
                                )
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .pl(px(2.0))
                                        .text_size(ui_text_md(cx))
                                        .text_color(p.text_muted)
                                        .child("s"),
                                ),
                        )
                        .child(div().w(px(1.0)).h(SPACE_XL).bg(p.border_subtle))
                        .child(
                            div()
                                .text_size(ui_text_md(cx))
                                .text_color(p.text_secondary)
                                .child(i18n!(cx, "sftp.commands.target.label")),
                        )
                        .child(Select::new(&self.target_select))
                        .child({
                            let max_tooltip = if self.is_maximized {
                                i18n!(cx, "sftp.commands.tooltip.restore")
                            } else {
                                i18n!(cx, "sftp.commands.tooltip.maximize")
                            };
                            let max_icon = if self.is_maximized {
                                AppIcon::FullscreenExit
                            } else {
                                AppIcon::Fullscreen
                            };
                            div()
                                .id("commands-maximize-btn")
                                .flex_shrink_0()
                                .w(px(24.0))
                                .h(px(24.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(RADIUS_STD)
                                .cursor_pointer()
                                .hover(|s| s.bg(rgb(t.bg_hover)))
                                .child(
                                    max_icon
                                        .size(ui_icon_std_ts(cx))
                                        .text_color(p.text_primary),
                                )
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.toggle_maximized(window, cx);
                                }))
                                .tooltip(move |_, cx| {
                                    let __tip = max_tooltip.clone();
                                    cx.new(|_| Tooltip::new(__tip)).into()
                                })
                        })
                        .child(
                            icon_button("commands-collapse", AppIcon::ChevronDown, &t, cx)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.toggle_collapsed(cx);
                                })),
                        ),
                );

        // ─── Body: command input + (optional) session picker ───
        let body = h_flex()
            .size_full()
            .gap(SPACE_XL)
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .flex()
                    .flex_col()
                    .min_h(px(0.0))
                    .overflow_hidden()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _event, window, cx| {
                            this.command_input.update(cx, |input, cx| {
                                input.focus(window, cx);
                            });
                            this.focus_manager.update(cx, |fm, cx| {
                                this.workspace.update(cx, |ws, cx| {
                                    ws.clear_focused_terminal(fm, cx);
                                });
                            });
                        }),
                    )
                    .child(
                        SimpleInput::new(&self.command_input)
                            .text_size(ui_text_md(cx))
                            .fill_height()
                            .appearance(false)
                            .focus_ring(false),
                    ),
            )
            .when(self.target_mode == "Selected Sessions", |el| {
                el.child(
                    div()
                        .w(px(240.0))
                        .h_full()
                        .bg(surface_bg(t.bg_secondary, cx))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .overflow_y_scrollbar()
                                .children({
                                    let session_order = self.session_order(cx);

                                    session_order.into_iter().enumerate().map(|(idx, tid)| {
                                        let is_checked = self.selected_sessions.contains(&tid);
                                        let display = conn_names
                                            .get(&tid)
                                            .cloned()
                                            .unwrap_or_else(|| tid.clone());
                                        let tid_clone = tid.clone();

                                        h_flex()
                                            .id(ElementId::Name(
                                                format!("session-chk-{}", idx).into(),
                                            ))
                                            .px(SPACE_MD)
                                            .py(SPACE_XS)
                                            .gap(SPACE_MD)
                                            .items_center()
                                            .cursor_pointer()
                                            .hover(|s| s.bg(rgb(t.bg_hover)))
                                            .child(
                                                div()
                                                    .w(px(14.0))
                                                    .h(px(14.0))
                                                    .border_1()
                                                    .border_color(p.border_subtle)
                                                    .rounded(RADIUS_MD)
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .bg(rgb(if is_checked {
                                                        t.bg_selection
                                                    } else {
                                                        t.bg_primary
                                                    }))
                                                    .when(is_checked, |el| {
                                                        el.child(
                                                                AppIcon::Check
                                                                .size(ICON_SM)
                                                                .text_color(p.text_primary),
                                                        )
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .text_size(ui_text_md(cx))
                                                    .text_color(p.text_primary)
                                                    .child(display),
                                            )
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if this.selected_sessions.contains(&tid_clone) {
                                                    this.selected_sessions.remove(&tid_clone);
                                                } else {
                                                    this.selected_sessions
                                                        .insert(tid_clone.clone());
                                                }
                                                cx.notify();
                                            }))
                                    })
                                }),
                        ),
                )
            })
            .into_any_element();

        let height_px = px(self.panel_height);

        // Draggable top edge (slim 1px line, turns blue on hover).
        let drag_entity = cx.entity().downgrade();
        let drag_entity_for_handle = drag_entity.clone();
        let resize_handle =
            ResizeHandle::new(true, t.border, t.border_active, move |pos, app_cx| {
                if let Some(entity) = drag_entity_for_handle.upgrade() {
                    entity.update(app_cx, |this, cx| {
                        // A manual resize replaces the maximized preset size.
                        this.is_maximized = false;
                        this.resize_dragging = Some(ResizeDragState {
                            start_y: f32::from(pos.y),
                            start_height: this.panel_height,
                        });
                        cx.notify();
                    });
                }
            });

        div()
            .id("commands-panel-root")
            .track_focus(&self.focus_handle)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(|_, _, cx| cx.stop_propagation())
            .w_full()
            .h(height_px)
            .flex()
            .flex_col()
            .relative()
            .child(resize_handle)
            .child(header)
            .child(body)
            .child(
                canvas(|_bounds, _window, _cx| {}, {
                    let ent = drag_entity.clone();
                    move |_bounds, _prepaint, window, _cx| {
                        let ent_move = ent.clone();
                        window.on_mouse_event(move |e: &MouseMoveEvent, phase, _window, cx| {
                            if phase != DispatchPhase::Bubble {
                                return;
                            }
                            if let Some(entity) = ent_move.upgrade() {
                                entity.update(cx, |this, cx| {
                                    if let Some(drag) = this.resize_dragging {
                                        let delta = drag.start_y - f32::from(e.position.y);
                                        let new_h = (drag.start_height + delta).clamp(120.0, 720.0);
                                        this.panel_height = new_h;
                                        cx.notify();
                                    }
                                });
                            }
                        });
                        let ent_up = ent.clone();
                        window.on_mouse_event(move |e: &MouseUpEvent, phase, _window, cx| {
                            if phase != DispatchPhase::Bubble {
                                return;
                            }
                            if e.button != MouseButton::Left {
                                return;
                            }
                            if let Some(entity) = ent_up.upgrade() {
                                entity.update(cx, |this, cx| {
                                    if this.resize_dragging.is_some() {
                                        this.resize_dragging = None;
                                        cx.notify();
                                    }
                                });
                            }
                        });
                    }
                })
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .h(SPACE_XS),
            )
            .into_any_element()
    }
}

impl Focusable for CommandsPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Panel for CommandsPanel {
    fn metadata(&self, cx: &App) -> PanelInfo {
        PanelInfo::new(
            "commands",
            i18n!(cx, "sftp.commands.title"),
            AppIcon::CommandAction,
            PanelKind::Custom,
        )
        .closable(false)
    }

    fn on_open(&mut self, cx: &mut Context<Self>) {
        self.commands_focus_pending = true;
        cx.notify();
    }

    fn on_focus(&mut self, cx: &mut Context<Self>) {
        self.commands_focus_pending = true;
        cx.notify();
    }

    fn on_show(&mut self, cx: &mut Context<Self>) {
        self.commands_focus_pending = true;
        cx.notify();
    }

    fn focus_handle(&self, cx: &App) -> Option<FocusHandle> {
        Some(self.command_input.read(cx).focus_handle(cx))
    }

    fn toolbar_elements(&self, cx: &App) -> Vec<ToolbarItem> {
        let this = self.self_weak.clone();

        let play_enabled = self.cmd_state != CommandExecutionState::Running;
        let pause_enabled = self.cmd_state == CommandExecutionState::Running;
        let stop_enabled = self.cmd_state != CommandExecutionState::Idle;

        let _target_label = match self.target_mode.as_str() {
            "Current Terminal" => i18n!(cx, "sftp.commands.target.current"),
            "Selected Sessions" => i18n!(cx, "sftp.commands.target.selected"),
            _ => i18n!(cx, "sftp.commands.target.all"),
        };

        // Helper: create an Arc callback from a WeakEntity + typed action fn.
        // Uses the entity update pattern to avoid closure-lifetime issues.
        fn make_click_cb<T: 'static>(
            weak: WeakEntity<T>,
            action: impl Fn(&mut T, &mut Context<T>) + Send + Sync + 'static,
        ) -> Arc<dyn Fn(&mut Window, &mut App) + Send + Sync> {
            Arc::new(move |_: &mut Window, cx: &mut App| {
                if let Some(slf) = weak.upgrade() {
                    let _ = slf.update(cx, |t, cx| action(t, cx));
                }
            })
        }

        let play_this = this.clone();
        let play_cb = make_click_cb(play_this, |t, cx| {
            t.run_commands(cx);
        });

        let pause_this = this.clone();
        let pause_cb = make_click_cb(pause_this, |t, cx| {
            t.pause_commands(cx);
        });

        let stop_this = this.clone();
        let stop_cb = make_click_cb(stop_this, |t, cx| {
            t.stop_commands(cx);
        });



        let mut items = vec![
            ToolbarItem::IconButton {
                icon: AppIcon::Play,
                tooltip: i18n!(cx, "sftp.commands.tooltip.run"),
                enabled: play_enabled,
                on_click: play_cb,
                accent: Some(AccentColor::Success),
                active: self.cmd_state == CommandExecutionState::Running,
            },
            ToolbarItem::IconButton {
                icon: AppIcon::Pause,
                tooltip: i18n!(cx, "sftp.commands.tooltip.pause"),
                enabled: pause_enabled,
                on_click: pause_cb,
                accent: Some(AccentColor::Warning),
                active: self.cmd_state == CommandExecutionState::Paused,
            },
            ToolbarItem::IconButton {
                icon: AppIcon::Stop,
                tooltip: i18n!(cx, "sftp.commands.tooltip.stop"),
                enabled: stop_enabled,
                on_click: stop_cb,
                accent: Some(AccentColor::Error),
                // Stop 是已停止态的"默认"状态，无需常驻红色高亮；
                // 红色仅作为 hover 预览（由 panel.rs 的 group_hover 提供）。
                active: false,
            },
            ToolbarItem::Separator,
            ToolbarItem::Custom({
                let line_label = i18n!(cx, "sftp.commands.send_line");
                let block_label = i18n!(cx, "sftp.commands.send_block");
                let line_tip = i18n!(cx, "sftp.commands.tooltip.send_line");
                let block_tip = i18n!(cx, "sftp.commands.tooltip.send_block");

                let this_for_toolbar = this.clone();
                RadioGroup::new("cmd-toolbar-send-mode-radio")
                    .mode(RadioMode::Button)
                    .compact()
                    .options(vec![
                        RadioOption::new("line".to_string(), line_label).tooltip(line_tip),
                        RadioOption::new("block".to_string(), block_label).tooltip(block_tip),
                    ])
                    .selected(Some(self.send_mode.clone()))
                    .on_change(move |val: &String, _window, cx| {
                        if let Some(entity) = this_for_toolbar.upgrade() {
                            let val = val.clone();
                            let _ = entity.update(cx, |this, cx| {
                                if this.send_mode != val {
                                    this.send_mode = val;
                                    cx.notify();
                                }
                            });
                        }
                    })
                    .into_any_element()
            }),
        ];

        if let (Some(repeat_input), Some(interval_input)) = (&self.repeat_input, &self.interval_input) {
            items.push(ToolbarItem::Separator);
            items.push(ToolbarItem::Label(i18n!(cx, "sftp.commands.repeat")));
            items.push(ToolbarItem::TextInput {
                entity: repeat_input.clone(),
                width_px: 60.0,
                suffix: None,
                on_enter: None,
            });
            items.push(ToolbarItem::Label(i18n!(cx, "sftp.commands.interval")));
            items.push(ToolbarItem::TextInput {
                entity: interval_input.clone(),
                width_px: 72.0,
                suffix: None,
                on_enter: None,
            });
        }

        items.push(ToolbarItem::Separator);
        items.push(ToolbarItem::Label(i18n!(cx, "sftp.commands.target.label")));
        items.push(ToolbarItem::Custom(
            Select::new(&self.target_select)
                .into_any_element(),
        ));

        items
    }
}

/// Walk the layout tree in reading order, collecting
/// `(terminal_id, project_id, shell_type)` for every terminal node. Order is
/// preserved (depth-first, children in declaration order) so callers can
/// present sessions in the same left-to-right / top-to-bottom reading order
/// that the tab strip and tab-list dropdown use.
fn collect_shell_types_ordered(
    node: &LayoutNode,
    project_id: &str,
    out: &mut Vec<(String, String, ShellType)>,
) {
    match node {
        LayoutNode::Terminal {
            terminal_id,
            shell_type,
            ..
        } => {
            if let Some(id) = terminal_id {
                out.push((id.clone(), project_id.to_string(), shell_type.clone()));
            }
        }
        LayoutNode::Split { children, .. } | LayoutNode::Tabs { children, .. } => {
            for child in children {
                collect_shell_types_ordered(child, project_id, out);
            }
        }
    }
}

impl Render for CommandsPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_toolbar_inputs(window, cx);
        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);

        let conn_names = self.session_connection_names(cx);
        let any_focused = self
            .command_input
            .read(cx)
            .focus_handle(cx)
            .is_focused(window)
            || self
                .repeat_input
                .as_ref()
                .map(|i| i.read(cx).focus_handle(cx).is_focused(window))
                .unwrap_or(false)
            || self
                .interval_input
                .as_ref()
                .map(|i| i.read(cx).focus_handle(cx).is_focused(window))
                .unwrap_or(false);

        if any_focused {
            let is_modal = self.focus_manager.read(cx).is_modal();
            if !is_modal {
                self.focus_manager.update(cx, |fm, cx| {
                    self.workspace.update(cx, |ws, cx| {
                        ws.clear_focused_terminal(fm, cx);
                    });
                });
            }
        }

        // Auto-focus the command input once after opening the panel.
        if self.commands_focus_pending {
            let fh = self.command_input.read(cx).focus_handle(cx);
            if !fh.is_focused(window) {
                window.focus(&fh, cx);
            }
            self.focus_manager.update(cx, |fm, cx| {
                self.workspace.update(cx, |ws, cx| {
                    ws.clear_focused_terminal(fm, cx);
                });
            });
            let entity = cx.entity().downgrade();
            window.on_next_frame(move |_window, cx| {
                if let Some(entity) = entity.upgrade() {
                    entity.update(cx, |this, cx| {
                        this.commands_focus_pending = false;
                        cx.notify();
                    });
                }
            });
        }

        // Body: command input + session picker
        let body = {
            let inner = h_flex()
                .size_full()
                .gap(SPACE_XL)
                .child(
                    div()
                        .flex_1()
                        .h_full()
                        .flex()
                        .flex_col()
                        .min_h(px(0.0))
                        .overflow_hidden()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _event, window, cx| {
                                this.command_input.update(cx, |input, cx| {
                                    input.focus(window, cx);
                                });
                                this.focus_manager.update(cx, |fm, cx| {
                                    this.workspace.update(cx, |ws, cx| {
                                        ws.clear_focused_terminal(fm, cx);
                                    });
                                });
                            }),
                        )
                        .child(
                            SimpleInput::new(&self.command_input)
                                .text_size(ui_text_md(cx))
                                .fill_height()
                                .appearance(false)
                                .focus_ring(false),
                        ),
                )
                .when(self.target_mode == "Selected Sessions", |el| {
                    el.child(
                        div()
                            .w(px(240.0))
                            .h_full()
                            .bg(surface_bg(t.bg_secondary, cx))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .overflow_y_scrollbar()
                                    .children({
                                        let session_order = self.session_order(cx);

                                        session_order.into_iter().enumerate().map(
                                            |(idx, tid)| {
                                                let is_checked =
                                                    self.selected_sessions.contains(&tid);
                                                let display = conn_names
                                                    .get(&tid)
                                                    .cloned()
                                                    .unwrap_or_else(|| tid.clone());
                                                let tid_clone = tid.clone();

                                                h_flex()
                                                    .id(ElementId::Name(
                                                        format!("session-chk-{}", idx).into(),
                                                    ))
                                                    .px(SPACE_MD)
                                                    .py(SPACE_XS)
                                                    .gap(SPACE_MD)
                                                    .items_center()
                                                    .cursor_pointer()
                                                    .hover(|s| s.bg(rgb(t.bg_hover)))
                                                    .child(
                                                        div()
                                                            .w(px(14.0))
                                                            .h(px(14.0))
                                                            .border_1()
                                                            .border_color(p.border_subtle)
                                                            .rounded(RADIUS_MD)
                                                            .flex()
                                                            .items_center()
                                                            .justify_center()
                                                            .bg(rgb(if is_checked {
                                                                t.bg_selection
                                                            } else {
                                                                t.bg_primary
                                                            }))
                                                            .when(is_checked, |el| {
                                                                el.child(
                                                                        AppIcon::Check
                                                                        .size(ICON_SM)
                                                                        .text_color(rgb(
                                                                            t.text_primary
                                                                        )),
                                                                )
                                                            }),
                                                    )
                                                    .child(
                                                        div()
                                                            .flex_1()
                                                            .text_size(ui_text_md(cx))
                                                            .text_color(p.text_primary)
                                                            .child(display),
                                                    )
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        if this
                                                            .selected_sessions
                                                            .contains(&tid_clone)
                                                        {
                                                            this.selected_sessions
                                                                .remove(&tid_clone);
                                                        } else {
                                                            this.selected_sessions
                                                                .insert(tid_clone.clone());
                                                        }
                                                        cx.notify();
                                                    }))
                                            },
                                        )
                                    }),
                            ),
                    )
                });
            inner.into_any_element()
        };

        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .child(div().flex_1().w_full().min_h(px(0.0)).child(body))
    }
}

/// Format multi-line commands with terminal-standard backslash continuation lines (`\`).
///
/// Converts multiple non-empty lines into a single logical command block by ensuring all lines
/// except the last end with a trailing ` \` and carriage return `\r`.
pub(crate) fn format_block_command(cmd: &str) -> String {
    let lines: Vec<&str> = cmd
        .lines()
        .map(|l| l.trim_end_matches(['\r', '\n']))
        .filter(|l| !l.trim().is_empty())
        .collect();

    if lines.is_empty() {
        return String::new();
    }

    let mut result = String::new();
    let last_idx = lines.len() - 1;

    for (idx, line) in lines.iter().enumerate() {
        let trimmed_right = line.trim_end();
        if idx == last_idx {
            result.push_str(trimmed_right);
            result.push('\r');
        } else {
            if trimmed_right.ends_with('\\') {
                result.push_str(trimmed_right);
            } else {
                result.push_str(trimmed_right);
                result.push_str(" \\");
            }
            result.push('\r');
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::format_block_command;

    #[test]
    fn test_format_block_command_two_lines() {
        let input = "df\n-lh";
        let output = format_block_command(input);
        assert_eq!(output, "df \\\r-lh\r");
    }

    #[test]
    fn test_format_block_command_already_with_backslash() {
        let input = "df \\\n-lh";
        let output = format_block_command(input);
        assert_eq!(output, "df \\\r-lh\r");
    }

    #[test]
    fn test_format_block_command_single_line() {
        let input = "df -lh";
        let output = format_block_command(input);
        assert_eq!(output, "df -lh\r");
    }

    #[test]
    fn test_format_block_command_multiple_lines_with_empty() {
        let input = "\n\ndf\n\n-lh\n\n";
        let output = format_block_command(input);
        assert_eq!(output, "df \\\r-lh\r");
    }
}

