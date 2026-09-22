use crate::keybindings::{ToggleCommandsPanel, ToggleLeftDock, ToggleRightToolbar, ToggleSftpPanel};
use crate::settings::settings_entity;
use crate::theme::{ThemeColors, surface_bg, theme};
use crate::ui::tokens::{
    ICON_LG, ICON_MD, ICON_MICRO, ICON_SM, ICON_STD, RADIUS_LG, RADIUS_MD, RADIUS_STD,
    RADIUS_XS, SPACE_2XS, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL, SPACE_XS,
    ui_height_status_bar, ui_space_card_gap, ui_space_window_padding, ui_text, ui_text_md, ui_text_ms, ui_text_sm,
};
use crate::views::overlays::dialogs::service_dialog::ServiceDialogMode;
use crate::views::panels::quick_commands_panel::send_command_to_focused_terminal;
use crate::views::panels::toast::ToastManager;
use crate::workspace::state::{LayoutNode, Workspace};
use gpui::prelude::*;
use gpui::*;
use gpui::{AnyElement, ClipboardItem, InteractiveElement};
use parking_lot::Mutex;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use velowork_core::shell::ShellType;
use velowork_extensions::{ExtensionInstance, ExtensionRegistry};
use velowork_i18n::i18n;
use velowork_updater::{GlobalUpdateInfo, UpdateStatus};
use velowork_monitor::{MonitorCollector, MonitorOptions, MonitorSnapshot};
use velowork_state::{ServiceDefinition, ServiceNode, ServiceOp, ServiceRuntimeState, ServiceStatus};
use crate::views::overlays::menus::service_context_menu::{ServiceMenuRequest, ServiceMenuTarget};
use velowork_terminal::backend::TerminalBackend;
use velowork_terminal::{GlobalServiceMonitorEngine, ServiceMonitorEngine};
use velowork_terminal::{SshMonitorSource, SshSessionHandle, TerminalsRegistry};
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::icon::AppIcon;
use velowork_ui::icon_button::icon_button;
use velowork_ui::input::Input;
use velowork_ui::scrollable::Scrollbar;
use velowork_ui::simple_input::{InputChangedEvent, SimpleInputState};
use velowork_ui::tooltip::{Tooltip, TooltipDirection};
use velowork_ui::{h_flex, v_flex};
use velowork_views_terminal::transfer_store::{GlobalTransferStore, TransferStatus, TransferStore};
use velowork_workspace::stores::GlobalServiceStore;
use velowork_workspace::stores::{ConnectionEvent, GlobalConnectionStore, GlobalSessionStore};

/// Refresh interval for system stats
const REFRESH_INTERVAL: Duration = Duration::from_secs(2);

/// Interval at which the monitor worker probes the active remote host.
const MONITOR_TICK: Duration = Duration::from_secs(1);
/// Debounce applied when the focused remote terminal (with monitoring enabled)
/// changes: the worker waits this long for the focus to settle before starting
/// to probe the new host, so rapidly flicking between tabs doesn't spawn /
/// rebuild collectors over and over and waste resources.
const MONITOR_SWITCH_DEBOUNCE: Duration = Duration::from_millis(600);

/// Cached system stats
#[derive(Clone, Default, PartialEq)]
struct DiskInfo {
    mount: String,
    fs: String,
    total_gb: f32,
    avail_gb: f32,
}

#[derive(Clone, Default, PartialEq)]
struct SystemStats {
    cpu_usage: f32,
    memory_used_gb: f32,
    memory_total_gb: f32,
    hostname: String,
    os_name: String,
    os_version: String,
    cpu_brand: String,
    cpu_cores: usize,
    process_count: usize,
    disks: Vec<DiskInfo>,
    net_rx_rate: f64, // bytes/s
    net_tx_rate: f64, // bytes/s
    // Monitor status-bar metrics (local machine).
    uptime_secs: u64,
    load_one: f64,
    load_five: f64,
    load_fifteen: f64,
    user_count: usize,
    kernel: String,
    arch: String,
    swap_used_gb: f32,
    swap_total_gb: f32,
}

/// Host metrics for the status bar, sourced from `velowork-monitor`
/// via an SSH-backed source over the focused terminal's live russh session.
struct HostMetrics {
    /// Committed active target — the SSH session the collector currently
    /// probes. There is exactly **one** active collector at any time, so a
    /// non-focused (but monitoring-enabled) remote terminal is implicitly
    /// *paused*: no collector is ever built for it and no SSH probe is issued,
    /// keeping its background resource usage at zero.
    target: Arc<Mutex<Option<SshSessionHandle>>>,
    /// Desired target requested by the UI layer on focus change. The worker
    /// only promotes `desired` into `target` once the focused terminal has
    /// been stable for [`MONITOR_SWITCH_DEBOUNCE`], so flicking rapidly between
    /// tabs doesn't spawn / rebuild collectors over and over.
    desired: Arc<Mutex<Option<SshSessionHandle>>>,
    /// Monotonic-clock reading of the last time `desired` changed (set by the
    /// UI thread, read by the worker for the debounce comparison).
    desired_changed_at: Arc<Mutex<Option<Instant>>>,
    /// Latest collected snapshot, refreshed by the worker thread.
    snapshot: Arc<Mutex<MonitorSnapshot>>,
    /// Gate for the background worker. True only when the focused terminal is
    /// a remote session whose config enables resource monitoring. When false
    /// the worker never builds a collector or issues any probe, so a local
    /// terminal — or a remote session with monitoring off — costs zero.
    enabled: Arc<Mutex<bool>>,
    /// Options controlling which metrics are collected (CPU, memory, disk).
    options: Arc<Mutex<MonitorOptions>>,
}

impl HostMetrics {
    /// Spawn the background worker that drives `MonitorCollector::tick()`
    /// on its own cadence and publishes the latest `MonitorSnapshot`.
    /// Rebuilds the collector whenever the SSH target changes (focus moved
    /// to a different terminal / host).
    ///
    /// The worker is fully gated by [`HostMetrics::enabled`]: when monitoring
    /// is off (local terminal, or a remote session that disabled resource
    /// monitoring in its config) it never builds a collector and never issues
    /// any probe, so there is no redundant resource consumption. There is no
    /// "local probe" fallback — a local terminal simply does nothing.
    fn spawn_worker(&self, cx: &App) {
        let target = self.target.clone();
        let snapshot = self.snapshot.clone();
        let enabled = self.enabled.clone();
        let options = self.options.clone();
        let desired = self.desired.clone();
        let desired_changed_at = self.desired_changed_at.clone();
        cx.background_executor()
            .spawn(async move {
                let mut collector: Option<MonitorCollector> = None;
                let mut current: Option<SshSessionHandle> = None;
                let mut initialized = false;
                loop {
                    smol::Timer::after(MONITOR_TICK).await;
                    // Monitoring disabled (local terminal, or a remote session that
                    // disabled resource monitoring): never run any collector or
                    // probe. Non-focused monitoring-enabled terminals are likewise
                    // never promoted to `target`, so they stay paused.
                    if !*enabled.lock() {
                        continue;
                    }
                    // Debounce: only promote `desired` to `target` once the focus
                    // has been stable for MONITOR_SWITCH_DEBOUNCE. Rapid tab switches
                    // keep pushing `desired_changed_at` forward, so the collector for
                    // every intermediate terminal is never built.
                    let desired_snap = desired.lock().clone();
                    let stable = match *desired_changed_at.lock() {
                        Some(t) => t.elapsed() >= MONITOR_SWITCH_DEBOUNCE,
                        None => true,
                    };
                    if stable {
                        let mut tgt = target.lock();
                        let changed = match (tgt.as_ref(), desired_snap.as_ref()) {
                            (Some(a), Some(b)) => !Arc::ptr_eq(a, b),
                            (None, None) => false,
                            _ => true,
                        };
                        if changed {
                            *tgt = desired_snap.clone();
                        }
                    }
                    let next = target.lock().clone();
                    // Nothing committed to probe (focus on a local / monitoring-off
                    // terminal, or still within the debounce window): skip collection
                    // so we don't start a task for a terminal the user is just
                    // passing through.
                    if next.is_none() {
                        if current.is_some() {
                            current = None;
                            collector = None;
                        }
                        continue;
                    }
                    let changed = !initialized
                        || match (current.as_ref(), next.as_ref()) {
                            (Some(a), Some(b)) => !Arc::ptr_eq(a, b),
                            _ => true,
                        };
                    if changed {
                        initialized = true;
                        let opts = *options.lock();
                        collector = Some(
                            MonitorCollector::new(Box::new(SshMonitorSource::new(
                                next.clone().unwrap(),
                            )))
                            .with_options(opts),
                        );
                        current = next;
                    }
                    if let Some(c) = collector.as_mut() {
                        let opts = *options.lock();
                        if c.options() != opts {
                            c.set_options(opts);
                        }
                        if let Ok(snap) = c.tick() {
                            *snapshot.lock() = snap;
                        }
                    }
                }
            })
            .detach();
    }
}

/// Which tab is shown inside the merged monitor popup.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MonitorTab {
    Resource,
    Service,
}

/// Status bar component showing system info and time
pub struct StatusBar {
    workspace: Entity<Workspace>,
    focus_manager: Entity<crate::workspace::focus::FocusManager>,
    backend: Arc<dyn TerminalBackend>,
    /// Host metrics driver (local probe or SSH-backed), published into `cache`.
    host_metrics: HostMetrics,
    /// Latest rendered metrics, refreshed by the timer from `host_metrics`.
    cache: Arc<Mutex<SystemStats>>,
    /// Activate functions cloned from registry (keyed by extension ID).
    activate_fns: Vec<(String, velowork_extensions::ActivateFn)>,
    /// Active extension instances. Dropping an instance deactivates the extension
    /// (cancels background tasks, releases views).
    active_extensions: HashMap<String, ExtensionInstance>,
    sidebar_open: bool,
    sidebar_covering: bool,
    right_sidebar_covering: bool,
    /// Which tab is active in the merged monitor popup (Resources / Services).
    monitor_tab: MonitorTab,
    /// Whether the server resource monitor popup is open.
    monitor_open: bool,
    /// Set on the monitor toggle's mouse-down and consumed by the popup's
    /// `on_mouse_down_out` so clicking the toggle while open doesn't
    /// first close (capture phase) and then re-open.
    monitor_suppress_close: bool,
    /// Bounds of the monitor trigger button in the status bar, used to anchor
    /// the popup cleanly above the button.
    monitor_bounds: Rc<RefCell<Bounds<Pixels>>>,
    /// Bounds of the status bar container itself, used to guarantee popups
    /// sit precisely above its top border.
    status_bar_bounds: Rc<RefCell<Bounds<Pixels>>>,
    /// Transfer store backing the status-bar transfer manager.
    transfer_store: Entity<TransferStore>,
    /// Bounds of the transfer button, captured via a canvas overlay,
    /// used to anchor the transfer popup above the button.
    transfer_bounds: Rc<RefCell<Bounds<Pixels>>>,
    /// Overlay manager owning the transfer popup slot (rendered at
    /// WindowView level, like context menus / color picker).
    overlay_manager: Entity<crate::views::overlays::overlay_manager::OverlayManager>,
    /// Per-column pixel widths for the disk table. Persisted only for the
    /// current run (in-memory); empty until the first render seeds defaults.
    disk_col_widths: Vec<f32>,
    /// Active disk-table column resize drag, if any.
    disk_col_drag: Option<DiskColDrag>,
    /// Filter for service monitor cards (All / Running / Stopped).
    service_filter: ServiceFilterState,
    /// Search input for filtering services by name/display name.
    service_search_input: Entity<SimpleInputState>,
    /// Set of service IDs currently undergoing a single-probe operation.
    service_probing_ids: HashSet<String>,
    /// Map of service IDs to active running operations (Start / Stop / Restart).
    service_operating_ids: HashMap<String, ServiceOp>,
    /// Whether a full-refresh probe is currently in progress.
    service_is_refreshing_all: bool,
    /// Active monitor panel resize drag, if any.
    monitor_panel_drag: Option<MonitorPanelDrag>,
    /// Live monitor panel width & height during dragging (or loaded from settings).
    monitor_panel_width: Option<f32>,
    monitor_panel_height: Option<f32>,
    /// Global settings-sync runtime state (syncing flag + last outcome),
    /// backing the `sync_status_btn`. `None` if the global isn't registered.
    sync_status: Option<Entity<crate::sync_engine::SyncStatusStore>>,
    /// Right dock panel (set lazily by WindowView once it is created), used
    /// by the "Open Service Manager" entry to reveal the services panel.
    right_dock: Option<Entity<velowork_ui::dock::panel::DockPanel>>,
    /// Shared PTY terminal registry, used to send unbound-service commands to
    /// the focused terminal (mirrors the service manager panel's behavior).
    terminals: TerminalsRegistry,
    /// Scroll handles for the monitor popup tabs (resources / services).
    monitor_scroll_handle: ScrollHandle,
    service_scroll_handle: ScrollHandle,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MonitorResizeHandle {
    Top,
    Left,
    TopLeft,
}

#[derive(Clone, Copy)]
struct MonitorPanelDrag {
    handle: MonitorResizeHandle,
    start_pos: Point<Pixels>,
    start_w: f32,
    start_h: f32,
}

const MONITOR_PANEL_MIN_W: f32 = 460.0;
const MONITOR_PANEL_MIN_H: f32 = 360.0;

/// In-flight state for a disk-table column resize.
#[derive(Clone, Copy)]
struct DiskColDrag {
    /// Index of the column whose right edge is being dragged.
    col: usize,
    /// Pointer x at drag start (window coordinates).
    start_x: Pixels,
    /// Column width (px) at drag start.
    start_w: f32,
}

/// Filter state for service monitor card list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ServiceFilterState {
    #[default]
    All,
    Running,
    Stopped,
}

/// Minimum / maximum width (px) a disk-table column may be resized to, so the
/// table can't collapse or blow out its layout.
const DISK_COL_MIN: f32 = 48.0;
const DISK_COL_MAX: f32 = 260.0;

/// Default per-column widths (px) for the disk table's resizable columns
/// (mount / filesystem / total / available). The trailing usage column flexes
/// to fill the remaining space and is not resizable.
fn default_disk_col_widths() -> Vec<f32> {
    vec![140.0, 90.0, 90.0, 90.0]
}

impl StatusBar {
    pub fn new(
        workspace: Entity<Workspace>,
        focus_manager: Entity<crate::workspace::focus::FocusManager>,
        backend: Arc<dyn TerminalBackend>,
        overlay_manager: Entity<crate::views::overlays::overlay_manager::OverlayManager>,
        terminals: TerminalsRegistry,
        cx: &mut Context<Self>,
    ) -> Self {
        // Host metrics engine: local probe by default; switches to an
        // SSH-backed source when the focused terminal has a live russh session.
        let host_metrics = HostMetrics {
            target: Arc::new(Mutex::new(None)),
            desired: Arc::new(Mutex::new(None)),
            desired_changed_at: Arc::new(Mutex::new(None)),
            snapshot: Arc::new(Mutex::new(MonitorSnapshot::default())),
            enabled: Arc::new(Mutex::new(false)),
            options: Arc::new(Mutex::new(MonitorOptions::default())),
        };
        host_metrics.spawn_worker(cx);

        let cache = Arc::new(Mutex::new(SystemStats::default()));

        // Start periodic refresh (pull the latest snapshot into the render cache)
        let cache_for_task = cache.clone();
        let snapshot_for_task = host_metrics.snapshot.clone();
        let enabled_for_task = host_metrics.enabled.clone();
        cx.spawn(async move |this: WeakEntity<StatusBar>, cx| {
            loop {
                smol::Timer::after(REFRESH_INTERVAL).await;

                // Local terminal / monitoring disabled: the worker never
                // publishes a real snapshot, so skip the pull and avoid
                // needlessly re-rendering the status bar every tick.
                if !*enabled_for_task.lock() {
                    continue;
                }

                // Pull the latest snapshot into the render cache.
                let stats = {
                    let snap = snapshot_for_task.lock();
                    snapshot_to_stats(&snap)
                };

                // Only notify when the rendered stats actually changed, so an
                // idle remote host (stable CPU/mem/disk) doesn't force a
                // full status-bar re-render every 2s.
                let changed = {
                    let mut cached = cache_for_task.lock();
                    if *cached == stats {
                        false
                    } else {
                        *cached = stats;
                        true
                    }
                };
                if !changed {
                    continue;
                }

                // Notify to re-render
                let result = this.update(cx, |_this, cx| {
                    cx.notify();
                });

                if result.is_err() {
                    break; // View was dropped
                }
            }
        })
        .detach();

        // Clone activate functions from the global registry.
        let activate_fns: Vec<_> = cx
            .try_global::<ExtensionRegistry>()
            .map(|registry| {
                registry
                    .extensions()
                    .iter()
                    .map(|ext| (ext.manifest.id.to_string(), ext.activate.clone()))
                    .collect()
            })
            .unwrap_or_default();

        // Activate initially enabled extensions
        let enabled = settings_entity(cx)
            .read(cx)
            .settings
            .enabled_extensions
            .clone();
        let active_extensions = Self::activate_extensions(&activate_fns, &enabled, cx);

        // Observe settings to sync extensions when enabled_extensions changes
        let settings = settings_entity(cx);
        cx.observe(&settings, |this, entity, cx| {
            let enabled = entity.read(cx).settings.enabled_extensions.clone();
            this.sync_extensions(&enabled, cx);
        })
        .detach();

        // Re-render when workspace changes (for focused project updates)
        cx.observe(&workspace, |this, _, cx| {
            this.sync_service_monitor_target(cx);
            cx.notify();
        })
        .detach();
        // Also re-render when focus state changes (focus_manager moved off Workspace in slice 03)
        cx.observe(&focus_manager, |this, _, cx| {
            this.sync_service_monitor_target(cx);
            cx.notify();
        })
        .detach();
        // Re-render when SSH connections open/close, so the SFTP button reflects
        // the focused connection's `enable_sftp` setting. A freshly connected
        // session that disabled SFTP must hide the button once its session id is
        // resolved (the SSH handshake completes after focus is set).
        {
            let connection_store = cx.global::<GlobalConnectionStore>().0.clone();
            cx.subscribe(&connection_store, |this, _, _: &ConnectionEvent, cx| {
                this.sync_service_monitor_target(cx);
                cx.notify();
            })
            .detach();
        }

        // Re-render whenever the transfer store changes (transfers
        // start / progress / finish) so the button + popup stay live.
        let transfer_store = cx.global::<GlobalTransferStore>().0.clone();
        cx.observe(&transfer_store, |_, _, cx| cx.notify()).detach();

        // Re-render whenever the service store changes (services added /
        // edited / removed, monitoring toggled) so the status-bar entry and
        // popup stay live.
        if let Some(service_store) = cx.try_global::<GlobalServiceStore>() {
            let service_store = service_store.0.clone();
            cx.observe(&service_store, |this, _, cx| {
                this.sync_service_monitor_target(cx);
                cx.notify();
            })
            .detach();
        }
        // Re-render whenever the service monitor engine reports new runtime
        // states (status polled), so the popup reflects live statuses.
        if let Some(engine) = cx.try_global::<GlobalServiceMonitorEngine>() {
            let engine = engine.0.clone();
            cx.observe(&engine, |_, _, cx| cx.notify()).detach();
        }

        // Start background service probe loop
        let engine_for_probe = cx
            .try_global::<GlobalServiceMonitorEngine>()
            .map(|e| e.0.clone());
        if let Some(engine) = engine_for_probe {
            cx.spawn(async move |this: WeakEntity<StatusBar>, cx| {
                loop {
                    smol::Timer::after(Duration::from_secs(3)).await;

                    let snapshot = this.update(cx, |this, cx| {
                        this.sync_service_monitor_target(cx);
                        if !engine.read(cx).is_enabled() {
                            return None;
                        }
                        Some(engine.read(cx).snapshot_for_probe())
                    });

                    let Ok(Some((_session_id, services, Some(handle)))) = snapshot else {
                        if snapshot.is_err() {
                            break;
                        }
                        continue;
                    };

                    if services.is_empty() {
                        continue;
                    }

                    let results = cx
                        .background_executor()
                        .spawn(async move { ServiceMonitorEngine::probe(&services, &handle) })
                        .await;

                    let _ = this.update(cx, |_this, cx| {
                        engine.update(cx, |e, cx| {
                            e.apply_results(results, cx);
                        });
                    });
                }
            })
            .detach();
        }

        // Re-render whenever the sync status changes (syncing starts / finishes,
        // outcome updates) so the sync button icon + dot stay live.
        let sync_status = crate::sync_engine::sync_status_store(cx);
        if let Some(store) = &sync_status {
            cx.observe(store, |_, _, cx| cx.notify()).detach();
        }

        let service_search_input = cx.new(|cx| {
            SimpleInputState::new(cx).placeholder(i18n!(cx, "service.search_placeholder"))
        });
        cx.subscribe(&service_search_input, |_, _, _: &InputChangedEvent, cx| {
            cx.notify();
        })
        .detach();

        Self {
            workspace,
            focus_manager,
            backend,
            host_metrics,
            cache,
            activate_fns,
            active_extensions,
            sidebar_open: true,
            sidebar_covering: false,
            right_sidebar_covering: false,
            monitor_tab: MonitorTab::Resource,
            monitor_open: false,
            monitor_suppress_close: false,
            monitor_bounds: Rc::new(RefCell::new(Bounds::default())),
            status_bar_bounds: Rc::new(RefCell::new(Bounds::default())),
            transfer_store,
            transfer_bounds: Rc::new(RefCell::new(Bounds::default())),
            overlay_manager,
            disk_col_widths: Vec::new(),
            disk_col_drag: None,
            service_filter: ServiceFilterState::All,
            service_search_input,
            service_probing_ids: HashSet::new(),
            service_operating_ids: HashMap::new(),
            service_is_refreshing_all: false,
            monitor_panel_drag: None,
            monitor_panel_width: None,
            monitor_panel_height: None,
            sync_status,
            right_dock: None,
            terminals,
            monitor_scroll_handle: ScrollHandle::new(),
            service_scroll_handle: ScrollHandle::new(),
        }
    }

    /// Lazily wired by `WindowView` once the right dock panel is created, so
    /// the "Open Service Manager" entry can reveal the services panel.
    pub fn set_right_dock(&mut self, dock: Option<Entity<velowork_ui::dock::panel::DockPanel>>) {
        self.right_dock = dock;
    }

    /// Activate extensions that are in the enabled set.
    fn activate_extensions(
        activate_fns: &[(String, velowork_extensions::ActivateFn)],
        enabled: &HashSet<String>,
        cx: &mut App,
    ) -> HashMap<String, ExtensionInstance> {
        activate_fns
            .iter()
            .filter(|(id, _)| enabled.contains(id.as_str()))
            .map(|(id, activate)| (id.clone(), activate(cx)))
            .collect()
    }

    /// Sync active extensions with the current enabled set.
    /// Activates newly enabled extensions, deactivates disabled ones
    /// (dropping the instance cancels background tasks and releases views).
    fn sync_extensions(&mut self, enabled: &HashSet<String>, cx: &mut Context<Self>) {
        // Deactivate disabled (drop instances → cancel tasks)
        self.active_extensions
            .retain(|id, _| enabled.contains(id.as_str()));

        // Activate newly enabled
        for (id, activate) in &self.activate_fns {
            if enabled.contains(id.as_str()) && !self.active_extensions.contains_key(id) {
                self.active_extensions.insert(id.clone(), activate(cx));
            }
        }

        cx.notify();
    }

    pub fn set_sidebar_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.sidebar_open != open {
            self.sidebar_open = open;
            cx.notify();
        }
    }

    pub fn set_sidebars_covering(&mut self, left: bool, right: bool, cx: &mut Context<Self>) {
        if self.sidebar_covering != left || self.right_sidebar_covering != right {
            self.sidebar_covering = left;
            self.right_sidebar_covering = right;
            cx.notify();
        }
    }
}

/// Lightweight 20px floating capsule icon button tailored for the 24px status bar.
///
/// Ensures strict 20px height (2px vertical breathing margin) and 24px width
/// so the leftmost icon center remains locked on the X = 20px alignment axis.
fn status_bar_icon_btn(
    id: impl Into<ElementId>,
    icon: impl Into<AppIcon>,
    custom_color: Option<Hsla>,
    t: &ThemeColors,
    cx: &App,
) -> Stateful<Div> {
    let p = SemanticPalette::from_context(cx);
    let el_id = id.into();
    let group_id = SharedString::from(format!("sb-btn-{:?}", el_id));
    let default_color = custom_color.unwrap_or(p.text_secondary);
    div()
        .id(el_id)
        .group(group_id.clone())
        .flex_shrink_0()
        .cursor_pointer()
        .w(px(24.0))
        .h(px(20.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(RADIUS_STD)
        .hover(|s| s.bg(surface_bg(t.bg_hover, cx)))
        .child(
            icon
                .into()
                .size(ICON_STD)
                .text_color(default_color)
                .group_hover(group_id, move |s| s.text_color(custom_color.unwrap_or(p.text_primary))),
        )
}

impl Render for StatusBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        // Severity / status indicator colors come from the design system's
        // semantic palette (single source of truth), not raw theme fields.
        let palette = SemanticPalette::from_context(cx);

        // Seed the disk-table column widths on first render (in-run persistence).
        if self.disk_col_widths.is_empty() {
            self.disk_col_widths = default_disk_col_widths();
        }

        // 1. Left Area: Toggle Left Sidebar, SFTP, Commands
        let has_focused_terminal = self
            .focus_manager
            .read(cx)
            .focused_terminal_state()
            .is_some();
        let sftp_label = i18n!(cx, "sftp.panel.title");
        let commands_label = i18n!(cx, "quick_commands.title");
        // Only show the SFTP button when the focused connection actually
        // supports SFTP. For an SSH terminal this means `enable_sftp` is set;
        // for a local terminal (no SSH session) we keep it visible as before.
        let show_sftp = if has_focused_terminal {
            match self.focused_sftp_state(cx) {
                Some(enabled) => enabled, // SSH terminal: its setting wins
                None => false,            // local terminal: keep visible
            }
        } else {
            false
        };
        let sftp_btn = if show_sftp {
            let trigger = status_bar_icon_btn("sftp-toggle-btn", AppIcon::Folder, None, &t, cx).on_click(
                |_, window, cx| {
                    window.dispatch_action(Box::new(ToggleSftpPanel), cx);
                },
            );
            Some(trigger.tooltip(move |_, cx| {
                cx.new(|_| Tooltip::new(sftp_label.clone()).direction(TooltipDirection::Top))
                    .into()
            }))
        } else {
            None
        };

        let commands_trigger = status_bar_icon_btn("commands-toggle-btn", AppIcon::CommandAction, None, &t, cx)
            .on_click(|_, window, cx| {
                window.dispatch_action(Box::new(ToggleCommandsPanel), cx);
            });
        let commands_btn = commands_trigger.tooltip(move |_, cx| {
            cx.new(|_| Tooltip::new(commands_label.clone()).direction(TooltipDirection::Top))
                .into()
        });

        let left_area = h_flex()
            .gap(SPACE_XS)
            .items_center()
            .child(
                status_bar_icon_btn("left-sidebar-toggle-btn", AppIcon::SidebarLeft, None, &t, cx)
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(ToggleLeftDock), cx);
                    })
                    .tooltip(move |_, cx| {
                        let tip = i18n!(cx, "status_bar.toggle_left_toolbar");
                        cx.new(|_| Tooltip::new(tip)).into()
                    }),
            )
            .children(sftp_btn)
            .child(commands_btn);

        // 2. Right Area: Encoding, Transfer, Sync status, Toggle Right Sidebar
        let charset_label = i18n!(cx, "status_bar.encoding");
        let _charset_selector = div()
            .id("charset-btn")
            .group("charset-btn")
            .cursor_pointer()
            .h(px(20.0))
            .px(SPACE_SM)
            .rounded(RADIUS_STD)
            .flex()
            .items_center()
            .hover(|s| s.bg(surface_bg(t.bg_hover, cx)))
            .child(
                h_flex()
                    .gap(SPACE_XS)
                    .items_center()
                    .child(
                        div()
                            .text_size(ui_text_sm(cx))
                            .text_color(rgb(t.text_primary))
                            .child("UTF-8"),
                    )
                    .child(
                        AppIcon::ChevronDown
                            .size(ICON_MICRO)
                            .text_color(rgb(t.text_muted))
                            .group_hover("charset-btn", |s| s.text_color(rgb(t.text_primary))),
                    ),
            )
            .tooltip(move |_, cx| {
                cx.new(|_| Tooltip::new(charset_label.clone()).direction(TooltipDirection::Top))
                    .into()
            });

        // 3. Transfer manager button (bottom-right). Hidden when there
        // are no transfers; icon color reflects aggregate status:
        //   active → green, error → red, paused → yellow, all done → gray.
        // No status text is shown next to the icon (per design).
        let transfer_indicator = self.transfer_store.read(cx).indicator();
        let transfers_btn = if let Some(status) = transfer_indicator {
            let icon_color = match status {
                TransferStatus::Active => palette.status_success,
                TransferStatus::Error => palette.status_error,
                TransferStatus::Paused => palette.status_warning,
                TransferStatus::Complete => palette.text_muted,
            };
            let transfers_label = i18n!(cx, "transfers.label");
            let transfer_bounds = self.transfer_bounds.clone();
            let trigger = div()
                .id("transfers-btn")
                .cursor_pointer()
                .h(px(20.0))
                .px(SPACE_SM)
                .rounded(RADIUS_STD)
                .flex()
                .items_center()
                .hover(|s| s.bg(surface_bg(t.bg_hover, cx)))
                .on_click(cx.listener(|this, _ev, _w, cx| {
                    // Anchor point: top-right of the button, popup opens above
                    // it (Anchor::BottomRight). Managed by OverlayManager so
                    // the popup renders at WindowView level with a proper
                    // click-outside backdrop, mutually exclusive with other
                    // overlays.
                    let b = *this.transfer_bounds.borrow();
                    let anchor = point(b.origin.x + b.size.width - px(6.0), b.origin.y);
                    this.overlay_manager.update(cx, |om, cx| {
                        om.toggle_transfer_popup(anchor, cx);
                    });
                }))
                .child(
                    AppIcon::Transfer
                        .size(ICON_STD)
                        .text_color(icon_color),
                )
                // Capture this button's bounds to anchor the popup above it.
                .child(
                    canvas(
                        {
                            let b = transfer_bounds.clone();
                            move |bounds, _window, _cx| {
                                *b.borrow_mut() = bounds;
                            }
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .size_full(),
                );
            Some(trigger.tooltip(move |_, cx| {
                cx.new(|_| Tooltip::new(transfers_label.clone()).direction(TooltipDirection::Top))
                    .into()
            }))
        } else {
            None
        };

        // Settings-sync status button. Completely hidden when sync is disabled.
        // When enabled: shows a colored dot reflecting the last sync outcome
        // (green = success, red = error, yellow = conflict; muted = never synced).
        // While a sync is running it swaps to a refresh icon. Clicking triggers a
        // manual sync. Hovering shows the last successful sync time.
        let sync_settings = settings_entity(cx).read(cx).settings.sync.clone();
        let sync_status_btn = if sync_settings.enabled {
            let (syncing, last_outcome) = self
                .sync_status
                .as_ref()
                .map(|s| {
                    let s = s.read(cx);
                    (s.syncing, s.last_outcome)
                })
                .unwrap_or((false, None));

            let last_sync_label = i18n!(cx, "settings.sync.last_sync");
            let never_label = i18n!(cx, "settings.sync.never");
            let syncing_label = i18n!(cx, "settings.sync.syncing");
            let tooltip_text = if syncing {
                syncing_label
            } else {
                match &sync_settings.last_sync_at {
                    Some(ts) => format!("{}: {}", last_sync_label, ts),
                    None => format!("{}: {}", last_sync_label, never_label),
                }
            };

            // While syncing: spinning refresh icon with smooth rotation animation.
            // Otherwise: sync icon + colored dot.
            let indicator = if syncing {
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .with_animation(
                        "status-bar-sync-spin",
                        Animation::new(Duration::from_millis(1000)).repeat(),
                        move |this, delta| {
                            let angle = delta * std::f32::consts::TAU;
                            this.child(
                                AppIcon::Refresh
                                    .svg()
                                    .size(ICON_STD)
                                    .text_color(palette.status_info)
                                    .with_transformation(Transformation::rotate(radians(angle))),
                            )
                        },
                    )
                    .into_any_element()
            } else {
                let dot_color = match last_outcome {
                    Some(crate::sync_engine::SyncOutcome::Success) => palette.status_success,
                    Some(crate::sync_engine::SyncOutcome::Error) => palette.status_error,
                    Some(crate::sync_engine::SyncOutcome::Conflict) => palette.status_warning,
                    None => palette.text_muted,
                };
                    h_flex()
                        .gap(SPACE_XS)
                        .items_center()
                        .child(
                            AppIcon::CloudUpload
                                .size(ICON_STD)
                                .text_color(rgb(t.text_secondary))
                                .group_hover("sync-status-btn", |s| s.text_color(rgb(t.text_primary))),
                        )
                    .child(div().w(px(6.0)).h(px(6.0)).rounded(RADIUS_MD).bg(dot_color))
                    .into_any_element()
            };

            let trigger = div()
                .id("sync-status-btn")
                .group("sync-status-btn")
                .cursor_pointer()
                .h(px(20.0))
                .px(SPACE_SM)
                .rounded(RADIUS_STD)
                .flex()
                .items_center()
                .hover(|s| s.bg(surface_bg(t.bg_hover, cx)))
                .child(indicator)
                .on_click(cx.listener(|_this, _ev, _w, cx| {
                    crate::sync_engine::trigger_manual_sync(cx);
                }));
            Some(trigger.tooltip(move |_, cx| {
                cx.new(|_| Tooltip::new(tooltip_text.clone()).direction(TooltipDirection::Top))
                    .into()
            }))
        } else {
            None
        };

        // App update indicator: shows when an update is downloaded and ready,
        // currently installing, or waiting to restart.
        let update_btn = cx
            .try_global::<GlobalUpdateInfo>()
            .and_then(|info| {
                let update = &info.0;
                if update.is_dismissed() {
                    return None;
                }
                let p = SemanticPalette::from_context(cx);
                match update.status() {
                    UpdateStatus::Ready { .. } => {
                        let tip = i18n!(cx, "update.available");
                        Some(
                            status_bar_icon_btn("update-ready-btn", AppIcon::Download, Some(p.status_success), &t, cx)
                                .on_click(|_, window, cx| {
                                    window.dispatch_action(Box::new(crate::keybindings::ShowUpdateDialog), cx);
                                })
                                .tooltip(move |_, cx| {
                                    cx.new(|_| Tooltip::new(tip.clone()).direction(TooltipDirection::Top)).into()
                                }),
                        )
                    }
                    UpdateStatus::Installing { version } => {
                        let text = format!("{}: v{}...", i18n!(cx, "update.installing"), version);
                        Some(
                            div()
                                .id("update-installing-badge")
                                .h(px(20.0))
                                .px(SPACE_SM)
                                .flex()
                                .items_center()
                                .rounded(RADIUS_STD)
                                .text_size(ui_text_sm(cx))
                                .text_color(p.status_warning)
                                .child(text),
                        )
                    }
                    UpdateStatus::ReadyToRestart { .. } => {
                        let tip = i18n!(cx, "update.restart_to_apply");
                        Some(
                            status_bar_icon_btn("update-restart-btn", AppIcon::Refresh, Some(p.status_success), &t, cx)
                                .on_click(|_, _window, cx| {
                                    velowork_updater::restart_app(cx);
                                })
                                .tooltip(move |_, cx| {
                                    cx.new(|_| Tooltip::new(tip.clone()).direction(TooltipDirection::Top)).into()
                                }),
                        )
                    }
                    _ => None,
                }
            });

        let right_toolbar_open = settings_entity(cx).read(cx).settings.right_toolbar_open;
        let toggle_right_sidebar_btn =
            status_bar_icon_btn("right-sidebar-toggle-btn", AppIcon::SidebarRight, None, &t, cx)
                .when(right_toolbar_open, |btn| {
                    btn.bg(surface_bg(t.bg_selection, cx))
                })
                .on_click(|_, window, cx| {
                    window.dispatch_action(Box::new(ToggleRightToolbar), cx);
                })
                .tooltip(move |_, cx| {
                    let tip = if right_toolbar_open {
                        i18n!(cx, "status_bar.hide_right_toolbar")
                    } else {
                        i18n!(cx, "status_bar.show_right_toolbar")
                    };
                    cx.new(|_| Tooltip::new(tip).direction(TooltipDirection::Top)).into()
                });

        let divider = || div().w(px(1.0)).h(px(10.0)).bg(palette.border_subtle);

        // Active session IP. We show the literal address the user configured
        // (e.g. "10.254.100.224") rather than a resolved peer name.
        //
        // An SSH session runs `ssh user@host` inside an ordinary project, so we
        // read the host straight from the focused terminal's shell args.
        // Local (non-remote) sessions fall back to "localhost".
        let active_ip = self
            .focus_manager
            .read(cx)
            .focused_terminal_state()
            .and_then(|ft| {
                // SSH terminal: parse `user@host` from the focused pane's shell.
                let layout = self
                    .workspace
                    .read(cx)
                    .project(&ft.project_id)?
                    .layout
                    .as_ref()?;
                let node = layout.get_at_path(&ft.layout_path)?;
                if let LayoutNode::Terminal { shell_type, .. } = node {
                    if let ShellType::Custom { path, args } = shell_type {
                        if path == "ssh" {
                            if let Some(host) = args
                                .iter()
                                .find(|a| a.contains('@'))
                                .and_then(|a| a.rsplit('@').next())
                                .filter(|h| !h.is_empty())
                            {
                                return Some(host.to_string());
                            }
                        }
                    }
                }
                None
            });
        let ip_display = active_ip.clone().unwrap_or_else(|| "localhost".to_string());
        let ip_label = i18n!(cx, "status.copy_ip");
        let ip_to_copy = ip_display.clone();
        let ip_trigger = div()
            .id("sb-ip")
            .group("sb-ip")
            .cursor_pointer()
            .h(px(20.0))
            .px(SPACE_SM)
            .rounded(RADIUS_STD)
            .flex()
            .items_center()
            .hover(|s| s.bg(surface_bg(t.bg_hover, cx)))
            .child(
                h_flex()
                    .gap(SPACE_XS)
                    .items_center()
                    .child(
                        AppIcon::Network
                            .size(ICON_STD)
                            .text_color(rgb(t.text_secondary))
                            .group_hover("sb-ip", |s| s.text_color(rgb(t.text_primary))),
                    )
                    .child(
                        div()
                            .text_size(ui_text_sm(cx))
                            .text_color(rgb(t.text_primary))
                            .child(ip_display.clone()),
                    ),
            )
            .on_click(cx.listener(move |_this, _ev, _window, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(ip_to_copy.clone()));
                ToastManager::success(i18n!(cx, "status.ip_copied"), cx);
            }));
        let ip_el = ip_trigger.tooltip(move |_, cx| {
            cx.new(|_| Tooltip::new(ip_label.clone()).direction(TooltipDirection::Top))
                .into()
        });

        // CPU / memory monitor summary. Clicking toggles the resource popup.
        // Point the metrics engine at the focused terminal's SSH session only
        // when monitoring is actually enabled for this (remote) session; the
        // worker then swaps to an SSH-backed source and starts collecting.
        // `show_monitor` gates both the UI and the background collection.
        let show_monitor = self.update_monitor_target(cx);
        let (_enable_monitor, monitor_cpu, monitor_mem, monitor_disk) =
            self.focused_monitor_config(cx);

        let stats = self.cache.lock().clone();
        let cpu_pct = stats.cpu_usage as i32;
        let mem_pct = if stats.memory_total_gb > 0.0 {
            (stats.memory_used_gb / stats.memory_total_gb * 100.0) as i32
        } else {
            0
        };
        let disk_pct = if let Some(root) = stats.disks.iter().find(|d| d.mount == "/") {
            if root.total_gb > 0.0 {
                ((root.total_gb - root.avail_gb) / root.total_gb * 100.0) as i32
            } else {
                0
            }
        } else if let Some(first) = stats.disks.first() {
            if first.total_gb > 0.0 {
                ((first.total_gb - first.avail_gb) / first.total_gb * 100.0) as i32
            } else {
                0
            }
        } else {
            0
        };
        let monitor_label = i18n!(cx, "status.monitor.title");
        let cpu_tooltip_label = i18n!(cx, "status.monitor.cpu");
        let mem_tooltip_label = i18n!(cx, "status.monitor.mem");
        let disk_tooltip_label = i18n!(cx, "status.monitor.disk");

        // Service overview for the status-bar monitor entry (running/total).
        // Computed independently of the service popover block below so the
        // summary is available before that block runs.
        let ov_focused = self.focus_manager.read(cx).focused_terminal_state();
        let ov_terminal_id = ov_focused.and_then(|f| {
            let project = self.workspace.read(cx).project(&f.project_id)?;
            let layout = project.layout.as_ref()?;
            match layout.get_at_path(&f.layout_path) {
                Some(LayoutNode::Terminal {
                    terminal_id: Some(tid),
                    ..
                }) => Some(tid),
                _ => None,
            }
        });
        let ov_session_id = ov_terminal_id
            .as_ref()
            .and_then(|tid| self.backend.get_ssh_session_id(tid));
        let ov_monitored: Vec<ServiceDefinition> = match &ov_session_id {
            Some(sid) => cx
                .try_global::<GlobalServiceStore>()
                .map(|s| s.0.read(cx).all_services())
                .unwrap_or_default()
                .into_iter()
                .filter(|s| s.session_id.as_deref() == Some(sid.as_str()) && s.monitor_enabled)
                .collect(),
            None => Vec::new(),
        };
        let ov_runtime: std::collections::HashMap<String, ServiceRuntimeState> = match (
            &ov_session_id,
            cx.try_global::<GlobalServiceMonitorEngine>(),
        ) {
            (Some(_), Some(engine)) => engine.0.read(cx).states().clone(),
            _ => std::collections::HashMap::new(),
        };
        let svc_overview_total = ov_monitored.len();
        let svc_overview_running = ov_monitored
            .iter()
            .filter(|s| {
                matches!(
                    ov_runtime.get(&s.id).map(|r| r.status),
                    Some(velowork_state::ServiceStatus::Running)
                )
            })
            .count();
        let svc_overview_label = i18n!(cx, "status.monitor.svc");
        let monitor_bounds = self.monitor_bounds.clone();
        let monitor_trigger = div()
            .id("sb-monitor-trigger")
            .cursor_pointer()
            .h(px(20.0))
            .px(SPACE_SM)
            .rounded(RADIUS_STD)
            .flex()
            .items_center()
            .hover(|s| s.bg(surface_bg(t.bg_hover, cx)))
            .child(
                canvas(
                    {
                        let b = monitor_bounds.clone();
                        move |bounds, _window, _cx| {
                            *b.borrow_mut() = bounds;
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(
                h_flex()
                    .gap(SPACE_MD)
                    .items_center()
                    .when(monitor_cpu, |el| {
                        el.child(monitor_item(
                            &t,
                            cx,
                            "sb-monitor-cpu",
                            AppIcon::Cpu,
                            cpu_pct,
                            cpu_tooltip_label,
                            window,
                        ))
                    })
                    .when(monitor_mem, |el| {
                        el.child(monitor_item(
                            &t,
                            cx,
                            "sb-monitor-mem",
                            AppIcon::Memory,
                            mem_pct,
                            mem_tooltip_label,
                            window,
                        ))
                    })
                    .when(monitor_disk, |el| {
                        el.child(monitor_item(
                            &t,
                            cx,
                            "sb-monitor-disk",
                            AppIcon::HardDrive,
                            disk_pct,
                            disk_tooltip_label,
                            window,
                        ))
                    })
                    .when(svc_overview_total > 0, |el| {
                        el.child(service_item(
                            &t,
                            cx,
                            "sb-monitor-svc",
                            AppIcon::SquareActivity,
                            svc_overview_running,
                            svc_overview_total,
                            svc_overview_label,
                            window,
                        ))
                    }),
            )
            .capture_any_mouse_down(cx.listener(|this, _ev, _w, _cx| {
                this.monitor_suppress_close = true;
            }))
            .capture_any_mouse_up(cx.listener(|this, _ev, _w, _cx| {
                this.monitor_suppress_close = false;
            }))
            .on_click(cx.listener(|this, _ev, _w, cx| {
                this.monitor_open = !this.monitor_open;
                if this.monitor_open {
                    this.monitor_tab = MonitorTab::Resource;
                    this.sync_service_monitor_target(cx);
                }
                cx.notify();
            }));
        let monitor_el = monitor_trigger.tooltip(move |_, cx| {
            cx.new(|_| Tooltip::new(monitor_label.clone()).direction(TooltipDirection::Top))
                .into()
        });

        let right_area = h_flex()
            .gap(SPACE_XS)
            .items_center()
            .child(ip_el)
            // .child(divider())
            // .child(charset_selector)
            .when(show_monitor, |el| el.child(divider()))
            .when(show_monitor, |el| el.child(monitor_el))
            .when_some(transfers_btn, |el, b| el.child(b))
            // .child(divider())
            .when_some(sync_status_btn, |el, b| el.child(b))
            .when_some(update_btn, |el, b| el.child(b))
            .child(toggle_right_sidebar_btn);

        let drag_entity = cx.entity().downgrade();
        let drag_overlay = if let Some(drag) = self.monitor_panel_drag {
            let cursor = match drag.handle {
                MonitorResizeHandle::Top => CursorStyle::ResizeUpDown,
                MonitorResizeHandle::Left => CursorStyle::ResizeLeftRight,
                MonitorResizeHandle::TopLeft => CursorStyle::ResizeUpLeftDownRight,
            };
            Some(
                canvas(
                    |_bounds, window, _cx| {
                        window.insert_hitbox(
                            Bounds::new(point(px(0.0), px(0.0)), window.viewport_size()),
                            HitboxBehavior::BlockMouseExceptScroll,
                        )
                    },
                    move |_bounds, hitbox, window, _cx| {
                        window.set_cursor_style(cursor, &hitbox);

                        let ent = drag_entity.clone();
                        window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
                            if phase != DispatchPhase::Bubble {
                                return;
                            }
                            if let Some(entity) = ent.upgrade() {
                                let _ = entity.update(cx, |this, cx| {
                                    if let Some(drag) = this.monitor_panel_drag {
                                        let viewport = window.viewport_size();
                                        let b = *this.monitor_bounds.borrow();
                                        let sb_b = *this.status_bar_bounds.borrow();
                                        let pos = Self::calc_monitor_anchor(viewport, b, sb_b, cx);

                                        let (max_w, max_h) = Self::calc_monitor_max_bounds(
                                            viewport, pos.x, pos.y, window, cx,
                                        );

                                        let delta_x = f32::from(e.position.x - drag.start_pos.x);
                                        let delta_y = f32::from(e.position.y - drag.start_pos.y);

                                        let new_w = match drag.handle {
                                            MonitorResizeHandle::Left
                                            | MonitorResizeHandle::TopLeft => {
                                                (drag.start_w - delta_x)
                                                    .clamp(MONITOR_PANEL_MIN_W, max_w)
                                            }
                                            _ => drag.start_w,
                                        };

                                        let new_h = match drag.handle {
                                            MonitorResizeHandle::Top
                                            | MonitorResizeHandle::TopLeft => {
                                                (drag.start_h - delta_y)
                                                    .clamp(MONITOR_PANEL_MIN_H, max_h)
                                            }
                                            _ => drag.start_h,
                                        };

                                        let changed = this.monitor_panel_width != Some(new_w)
                                            || this.monitor_panel_height != Some(new_h);
                                        if changed {
                                            this.monitor_panel_width = Some(new_w);
                                            this.monitor_panel_height = Some(new_h);
                                            cx.notify();
                                        }
                                    }
                                });
                            }
                        });

                        let ent_up = drag_entity.clone();
                        window.on_mouse_event(move |e: &MouseUpEvent, phase, _window, cx| {
                            if phase != DispatchPhase::Bubble {
                                return;
                            }
                            if e.button != MouseButton::Left {
                                return;
                            }
                            if let Some(entity) = ent_up.upgrade() {
                                let _ = entity.update(cx, |this, cx| {
                                    if let Some(_drag) = this.monitor_panel_drag.take() {
                                        if let (Some(w), Some(h)) =
                                            (this.monitor_panel_width, this.monitor_panel_height)
                                        {
                                            settings_entity(cx).update(cx, |s, cx| {
                                                s.set_monitor_popup_size(w, h, cx);
                                            });
                                        }
                                        cx.notify();
                                    }
                                });
                            }
                        });
                    },
                )
                .absolute()
                .size_full(),
            )
        } else {
            None
        };

        let is_custom_titlebar = if cfg!(target_os = "macos") || cfg!(target_os = "windows") {
            settings_entity(cx).read(cx).settings.titlebar_style
                == velowork_workspace::settings::TitlebarStyle::Custom
        } else {
            matches!(
                window.window_decorations(),
                gpui::Decorations::Client { .. }
            )
        };
        let window_corner_radius = settings_entity(cx).read(cx).settings.window_corner_radius;
        let is_maximized = window.is_maximized();
        let is_fullscreen = window.is_fullscreen();
        let has_rounded_corners =
            is_custom_titlebar && !is_maximized && !is_fullscreen && window_corner_radius > 0.0;
        let radius = px(window_corner_radius);

        let p = SemanticPalette::from_context(cx);

        let sb_bounds = self.status_bar_bounds.clone();
        div()
            .id("status-bar")
            .h(ui_height_status_bar(cx))
            .px(SPACE_MD)
            .flex()
            .items_center()
            .justify_between()
            .bg(p.surface_card)
            .border_t_1()
            .border_color(p.border_subtle)
            .child(
                canvas(
                    move |bounds, _window, _cx| {
                        *sb_bounds.borrow_mut() = bounds;
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .when(has_rounded_corners, |d| {
                d.rounded_bl(radius).rounded_br(radius)
            })
            .child(left_area)
            .child(right_area)
            .when_some(drag_overlay, |el, d| el.child(d))
    }
}

impl StatusBar {
    /// Render the monitor popup element for **window-level** rendering.
    ///
    /// This must be called from the window root (e.g. in `render.rs`), NOT
    /// inside the status bar's own `render()`, so that `deferred(anchored())`
    /// uses true window coordinates. This guarantees:
    /// - The popup bottom aligns exactly with the status bar top edge (no overlap)
    /// - The popup right edge stays within the window bounds (no overflow)
    pub fn render_monitor_popup(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let show_monitor = self.update_monitor_target(cx);
        if !self.monitor_open || !show_monitor {
            return None;
        }

        let t = theme(cx);

        // Gather the same data the popup needs — same sources as render().
        let stats = self.cache.lock().clone();
        let ip_display = self
            .focus_manager
            .read(cx)
            .focused_terminal_state()
            .and_then(|ft| {
                let layout = self
                    .workspace
                    .read(cx)
                    .project(&ft.project_id)?
                    .layout
                    .as_ref()?;
                let node = layout.get_at_path(&ft.layout_path)?;
                if let LayoutNode::Terminal { shell_type, .. } = node {
                    if let ShellType::Custom { path, args } = shell_type {
                        if path == "ssh" {
                            if let Some(host) = args
                                .iter()
                                .find(|a| a.contains('@'))
                                .and_then(|a| a.rsplit('@').next())
                                .filter(|h| !h.is_empty())
                            {
                                return Some(host.to_string());
                            }
                        }
                    }
                }
                None
            })
            .unwrap_or_else(|| "localhost".to_string());

        let focused = self.focus_manager.read(cx).focused_terminal_state();
        let terminal_id = focused.and_then(|f| {
            let project = self.workspace.read(cx).project(&f.project_id)?;
            let layout = project.layout.as_ref()?;
            match layout.get_at_path(&f.layout_path) {
                Some(LayoutNode::Terminal {
                    terminal_id: Some(tid),
                    ..
                }) => Some(tid),
                _ => None,
            }
        });
        let session_id = terminal_id
            .as_ref()
            .and_then(|tid| self.backend.get_ssh_session_id(tid));
        let all_services: Vec<ServiceDefinition> = cx
            .try_global::<GlobalServiceStore>()
            .map(|s| s.0.read(cx).all_services())
            .unwrap_or_default();
        let monitored_services: Vec<ServiceDefinition> = match &session_id {
            Some(sid) => all_services
                .iter()
                .filter(|s| s.session_id.as_deref() == Some(sid.as_str()) && s.monitor_enabled)
                .cloned()
                .collect(),
            None => Vec::new(),
        };
        let runtime_by_id: std::collections::HashMap<String, ServiceRuntimeState> =
            match (&session_id, cx.try_global::<GlobalServiceMonitorEngine>()) {
                (Some(_), Some(engine)) => engine.0.read(cx).states().clone(),
                _ => std::collections::HashMap::new(),
            };

        let viewport = window.viewport_size();
        let b = *self.monitor_bounds.borrow();
        let sb_b = *self.status_bar_bounds.borrow();
        let pos = Self::calc_monitor_anchor(viewport, b, sb_b, cx);

        Some(
            div()
                .absolute()
                .inset_0()
                .id("monitor-popup-backdrop")
                .child(deferred(
                    anchored()
                        .position(pos)
                        .anchor(gpui::Anchor::BottomRight)
                        .child(self.render_monitor_panel(
                            &t,
                            &stats,
                            &ip_display,
                            &monitored_services,
                            &runtime_by_id,
                            window,
                            cx,
                        )),
                ))
                .into_any_element(),
        )
    }
}


impl StatusBar {
    /// Detect the effective title bar height for the active window configuration.
    fn titlebar_height(window: &Window, cx: &App) -> f32 {
        let settings = settings_entity(cx).read(cx).settings.clone();
        let scale = velowork_ui::tokens::ui_scale_factor(cx);
        let is_custom_titlebar = if cfg!(target_os = "macos") {
            settings.titlebar_style == velowork_workspace::settings::TitlebarStyle::Custom
        } else {
            settings.titlebar_style == velowork_workspace::settings::TitlebarStyle::Custom
                || matches!(window.window_decorations(), Decorations::Client { .. })
        };
        if is_custom_titlebar && (!cfg!(target_os = "macos") || !window.is_fullscreen()) {
            settings.titlebar_height * scale
        } else {
            0.0
        }
    }

    /// Compute the monitor popup's anchor point (BottomRight).
    /// Guarantees that the panel bottom sits strictly SPACE_CARD_GAP above the
    /// status bar's top border in global window coordinates, perfectly colinear
    /// with Toast notifications and the right toolbar's bottom baseline.
    /// Guarantees that the panel's right edge aligns with the trigger button,
    /// clamped so it always stays at least SPACE_CARD_GAP away from the window right boundary.
    fn calc_monitor_anchor(
        viewport: Size<Pixels>,
        b: Bounds<Pixels>,
        sb_bounds: Bounds<Pixels>,
        cx: &App,
    ) -> Point<Pixels> {
        let card_gap = ui_space_card_gap(cx);
        let win_pad = ui_space_window_padding(cx);
        let anchor_x = if b.size.width > px(0.0) {
            (b.origin.x + b.size.width).min(viewport.width - win_pad)
        } else {
            viewport.width - win_pad
        };
        let anchor_y = if sb_bounds.size.height > px(0.0) && sb_bounds.origin.y > px(0.0) {
            sb_bounds.origin.y - card_gap
        } else if b.origin.y > px(0.0) {
            b.origin.y - card_gap
        } else {
            let status_bar_h = ui_height_status_bar(cx);
            viewport.height - status_bar_h - card_gap
        };
        point(anchor_x, anchor_y)
    }

    /// Compute the dynamic maximum width and height bounds for the monitor popup in global window space.
    /// Guarantees that:
    /// 1. The panel top never touches or covers the title bar (maintaining at least a 16px generous buffer below the title bar).
    /// 2. The panel bottom sits directly SPACE_CARD_GAP above the status bar without any gap drift.
    fn calc_monitor_max_bounds(
        viewport: Size<Pixels>,
        anchor_x: Pixels,
        anchor_y: Pixels,
        window: &Window,
        cx: &App,
    ) -> (f32, f32) {
        let title_bar_h = Self::titlebar_height(window, cx);
        let status_bar_h = f32::from(ui_height_status_bar(cx));
        let card_gap = f32::from(ui_space_card_gap(cx));
        let top_safe_limit = title_bar_h + 16.0;

        let max_w = (f32::from(anchor_x) - 16.0)
            .clamp(MONITOR_PANEL_MIN_W, (f32::from(viewport.width) - 32.0).max(MONITOR_PANEL_MIN_W));
        let max_h = (f32::from(anchor_y) - top_safe_limit)
            .clamp(
                MONITOR_PANEL_MIN_H,
                (f32::from(viewport.height) - top_safe_limit - status_bar_h - card_gap).max(MONITOR_PANEL_MIN_H),
            );

        (max_w, max_h)
    }

    /// Resolve the focused terminal's live russh session handle, if it is an
    /// SSH terminal. Mirrors the resolution in the SFTP panel.
    fn focused_ssh_session(&self, cx: &App) -> Option<SshSessionHandle> {
        let focused = self.focus_manager.read(cx).focused_terminal_state()?;
        let project = self.workspace.read(cx).project(&focused.project_id)?;
        let layout = project.layout.as_ref()?;
        match layout.get_at_path(&focused.layout_path) {
            Some(LayoutNode::Terminal {
                terminal_id: Some(tid),
                ..
            }) => self.backend.get_ssh_session(&tid),
            _ => None,
        }
    }

    /// For the focused terminal, report whether the SFTP toggle button should
    /// be shown.
    ///
    /// - SSH terminal: `Some(enable_sftp)` — only show when the connection has
    ///   SFTP enabled (so a connection that disabled SFTP hides the button).
    /// - Local terminal / nothing focused: `None` — caller decides (kept
    ///   visible to preserve prior behaviour).
    fn focused_sftp_state(&self, cx: &App) -> Option<bool> {
        let focused = self.focus_manager.read(cx).focused_terminal_state()?;
        let project = self.workspace.read(cx).project(&focused.project_id)?;
        let layout = project.layout.as_ref()?;
        let (terminal_id, shell_type) = match layout.get_at_path(&focused.layout_path) {
            Some(LayoutNode::Terminal {
                terminal_id: Some(tid),
                shell_type,
                ..
            }) => (tid, shell_type),
            _ => return None,
        };
        // Check if it's an SSH terminal by shell_type or session_id
        let is_ssh = matches!(shell_type, ShellType::Custom { path, .. } if path == "ssh")
            || self.backend.get_ssh_session_id(terminal_id).is_some();
        if !is_ssh {
            return None;
        }

        if let Some(session_id) = self.backend.get_ssh_session_id(terminal_id) {
            if let Some(store) = cx.try_global::<GlobalSessionStore>() {
                let session_store = store.0.read(cx);
                if let Some(session) = session_store.find_session(&session_id) {
                    return Some(session.enable_sftp);
                }
            }
        }

        // Ad-hoc / quick-connect SSH sessions default to enabled
        Some(true)
    }

    /// For the focused terminal, report whether the server resource monitor
    /// (UI + background collection) should be active, along with fine-grained
    /// toggle states for CPU, memory, and disk.
    ///
    /// Returns `(enable_monitor, monitor_cpu, monitor_mem, monitor_disk)`.
    fn focused_monitor_config(&self, cx: &App) -> (bool, bool, bool, bool) {
        let Some(focused) = self.focus_manager.read(cx).focused_terminal_state() else {
            return (false, false, false, false);
        };
        let Some(project) = self.workspace.read(cx).project(&focused.project_id) else {
            return (false, false, false, false);
        };
        let Some(layout) = project.layout.as_ref() else {
            return (false, false, false, false);
        };
        let terminal_id = match layout.get_at_path(&focused.layout_path) {
            Some(LayoutNode::Terminal {
                terminal_id: Some(tid),
                ..
            }) => tid,
            _ => return (false, false, false, false),
        };
        // Local terminals have no SSH session id → `None` here means "not
        // remote"; remote terminals resolve to their session's monitor settings.
        let Some(session_id) = self.backend.get_ssh_session_id(terminal_id) else {
            return (false, false, false, false);
        };
        let session_store = cx.global::<GlobalSessionStore>().0.read(cx);
        session_store
            .find_session(&session_id)
            .map_or((false, false, false, false), |s| {
                (
                    s.enable_monitor,
                    s.monitor_cpu,
                    s.monitor_mem,
                    s.monitor_disk,
                )
            })
    }

    /// Update the metrics engine's target from the focused terminal. Cheap
    /// (Arc pointer compare); the worker promotes it to the active collector
    /// once the focus has settled (debounce). Returns whether monitoring is
    /// active (remote session with monitoring enabled), which the caller uses
    /// to gate the status-bar UI.
    fn update_monitor_target(&self, cx: &App) -> bool {
        let (enable_monitor, monitor_cpu, monitor_mem, monitor_disk) =
            self.focused_monitor_config(cx);
        let enabled = enable_monitor && (monitor_cpu || monitor_mem || monitor_disk);
        *self.host_metrics.enabled.lock() = enabled;
        *self.host_metrics.options.lock() = MonitorOptions {
            enable_cpu: monitor_cpu,
            enable_mem: monitor_mem,
            enable_disk: monitor_disk,
        };
        let next = if enabled {
            self.focused_ssh_session(cx)
        } else {
            None
        };
        // Only bump the debounce timer when the desired terminal actually
        // changes. Frequent re-renders (the 2s refresh tick, focus observers)
        // must NOT reset it, otherwise the worker would keep deferring the
        // commit and the monitor would never start.
        let mut cur = self.host_metrics.desired.lock();
        let changed = match (cur.as_ref(), next.as_ref()) {
            (Some(a), Some(b)) => !Arc::ptr_eq(a, b),
            (None, None) => false,
            _ => true,
        };
        if changed {
            *cur = next;
            *self.host_metrics.desired_changed_at.lock() = Some(Instant::now());
        }
        enabled
    }

    /// Resolve the focused terminal's SSH session ID and live handle.
    fn focused_ssh_info(&self, cx: &App) -> (Option<String>, Option<SshSessionHandle>) {
        let Some(focused) = self.focus_manager.read(cx).focused_terminal_state() else {
            return (None, None);
        };
        let Some(project) = self.workspace.read(cx).project(&focused.project_id) else {
            return (None, None);
        };
        let Some(layout) = project.layout.as_ref() else {
            return (None, None);
        };
        let terminal_id = match layout.get_at_path(&focused.layout_path) {
            Some(LayoutNode::Terminal {
                terminal_id: Some(tid),
                ..
            }) => tid,
            _ => return (None, None),
        };
        let session_id = self.backend.get_ssh_session_id(terminal_id);
        let session_handle = self.backend.get_ssh_session(terminal_id);
        (session_id, session_handle)
    }

    /// Synchronize the ServiceMonitorEngine's target session and service list
    /// with the currently focused SSH terminal.
    fn sync_service_monitor_target(&self, cx: &mut App) {
        let (session_id, session_handle) = self.focused_ssh_info(cx);
        let all_services: Vec<ServiceDefinition> = cx
            .try_global::<GlobalServiceStore>()
            .map(|s| s.0.read(cx).all_services())
            .unwrap_or_default();

        let matched_services: Vec<ServiceDefinition> = match &session_id {
            Some(sid) => all_services
                .into_iter()
                .filter(|s| s.session_id.as_deref() == Some(sid.as_str()) && s.monitor_enabled)
                .collect(),
            None => Vec::new(),
        };

        if let Some(engine) = cx
            .try_global::<GlobalServiceMonitorEngine>()
            .map(|e| e.0.clone())
        {
            engine.update(cx, |e, cx| {
                if let (Some(sid), Some(handle)) = (session_id, session_handle) {
                    if e.session_id() != Some(&sid) || e.states().len() != matched_services.len() {
                        e.set_target(Some(sid), matched_services, cx);
                    }
                    e.set_session(handle);
                } else {
                    e.clear_session(cx);
                }
            });
        }
    }
}

/// Threshold color shared by `meter_bar` and any percentage text that should
/// color-code the same severity (green → yellow → red).
fn meter_color(t: &ThemeColors, pct: i32) -> u32 {
    if pct >= 80 {
        t.error
    } else if pct >= 60 {
        t.warning
    } else {
        t.success
    }
}

/// Small horizontal meter used in the status-bar monitor summary and the
/// resource panel cards.
fn meter_bar(t: &ThemeColors, pct: i32) -> impl IntoElement {
    let frac = (pct.clamp(0, 100) as f32) / 100.0;
    let color = meter_color(t, pct);
    div()
        .w_full()
        .h(px(6.0))
        .rounded(RADIUS_MD)
        .bg(rgb(t.border))
        .child(
            div()
                .h_full()
                .w(relative(frac))
                .bg(rgb(color))
                .rounded(RADIUS_MD),
        )
}

/// One monitor summary chip (icon + percentage + mini bar) in the status bar.
fn monitor_item(
    t: &ThemeColors,
    cx: &mut App,
    id: &'static str,
    icon: AppIcon,
    pct: i32,
    label: String,
    _window: &mut Window,
) -> impl IntoElement {
    let trigger = h_flex()
        .id(id)
        .gap(px(3.0))
        .items_center()
        .child(
            icon.size(ICON_STD)
                .text_color(rgb(t.text_secondary))
                .group_hover("sb-monitor", |s| s.text_color(rgb(t.text_primary))),
        )
        .child(
            div()
                .text_size(ui_text_sm(cx))
                .text_color(rgb(t.text_primary))
                .child(format!("{}%", pct)),
        )
        .child(
            div()
                .w(px(28.0))
                .h(px(3.0))
                .rounded(RADIUS_XS)
                .bg(rgb(t.border))
                .child(
                    div()
                        .h_full()
                        .w(relative((pct.clamp(0, 100) as f32) / 100.0))
                        .bg(rgb(if pct >= 80 {
                            t.error
                        } else if pct >= 60 {
                            t.warning
                        } else {
                            t.success
                        }))
                        .rounded(RADIUS_XS),
                ),
        );
    let tooltip_label = format!("{}: {}%", label, pct);
    trigger.tooltip(move |_, cx| {
        cx.new(|_| Tooltip::new(tooltip_label.clone()).direction(TooltipDirection::Top))
            .into()
    })
}

/// Service overview item on the status bar, mirroring `monitor_item` (icon +
/// ratio text + progress bar) but showing the running/total service counts.
/// The bar fills by `running / total` and uses the success color when at least
/// one service is running (a healthy state), falling back to a muted color.
fn service_item(
    t: &ThemeColors,
    cx: &mut App,
    id: &'static str,
    icon: AppIcon,
    running: usize,
    total: usize,
    label: String,
    _window: &mut Window,
) -> impl IntoElement {
    let _pct = if total > 0 {
        (running as f32 / total as f32 * 100.0) as i32
    } else {
        0
    };
    let ratio = if total > 0 {
        running as f32 / total as f32
    } else {
        0.0
    };
    let bar_color = if running > 0 { t.success } else { t.text_muted };
    let text = if total == 0 {
        i18n!(cx, "status.monitor.svc_none")
    } else {
        format!("{}/{}", running, total)
    };
    let tooltip_label = format!("{} {}/{}", label, running, total);
    let trigger = h_flex()
        .id(id)
        .gap(px(3.0))
        .items_center()
        .child(
            icon.size(ICON_STD)
                .text_color(rgb(t.text_secondary))
                .group_hover("sb-monitor", |s| s.text_color(rgb(t.text_primary))),
        )
        .child(
            div()
                .text_size(ui_text_sm(cx))
                .text_color(rgb(t.text_primary))
                .child(text),
        )
        .child(
            div()
                .w(px(28.0))
                .h(px(3.0))
                .rounded(RADIUS_XS)
                .bg(rgb(t.border))
                .child(
                    div()
                        .h_full()
                        .w(relative(ratio))
                        .bg(rgb(bar_color))
                        .rounded(RADIUS_XS),
                ),
        );
    trigger.tooltip(move |_, cx| {
        cx.new(|_| Tooltip::new(tooltip_label.clone()).direction(TooltipDirection::Top))
            .into()
    })
}

/// Format a GB value with one decimal.
fn fmt_gb(g: f32) -> String {
    format!("{:.1} GB", g)
}

/// Convert a `MonitorSnapshot` (local or SSH-sourced) into the render
/// model. The status bar's existing render code is untouched — it keeps
/// reading `SystemStats`.
fn snapshot_to_stats(s: &MonitorSnapshot) -> SystemStats {
    const GB: f64 = 1_073_741_824.0; // 1024^3
    SystemStats {
        cpu_usage: s.cpu.usage_pct,
        memory_used_gb: (s.memory.used_bytes as f64 / GB) as f32,
        memory_total_gb: (s.memory.total_bytes as f64 / GB) as f32,
        hostname: s.host.hostname.clone(),
        os_name: s.host.os_name.clone(),
        os_version: s.host.os_version.clone(),
        cpu_brand: s.host.cpu_brand.clone(),
        cpu_cores: s.host.cpu_cores as usize,
        process_count: s.processes as usize,
        disks: s
            .disks
            .iter()
            .map(|d| DiskInfo {
                mount: d.mount.clone(),
                fs: d.filesystem.clone(),
                total_gb: (d.total_bytes as f64 / GB) as f32,
                avail_gb: (d.available_bytes as f64 / GB) as f32,
            })
            .collect(),
        net_rx_rate: s.network.rx_rate,
        net_tx_rate: s.network.tx_rate,
        uptime_secs: s.uptime_secs,
        load_one: s.load.one,
        load_five: s.load.five,
        load_fifteen: s.load.fifteen,
        user_count: s.users as usize,
        kernel: s.host.kernel.clone(),
        arch: s.host.arch.clone(),
        swap_used_gb: (s.memory.swap_used_bytes as f64 / GB) as f32,
        swap_total_gb: (s.memory.swap_total_bytes as f64 / GB) as f32,
    }
}

/// A titled resource card used inside the monitor popup.
/// Every card shares the same width (`flex_1`) and fixed height so the
/// System / CPU / Memory / Network blocks are visually identical.
fn monitor_card(
    t: &ThemeColors,
    cx: &App,
    title: String,
    icon: AppIcon,
    body: AnyElement,
) -> AnyElement {
    let p = SemanticPalette::from_context(cx);
    v_flex()
        .flex_1()
        .overflow_hidden()
        .gap(px(10.0))
        .p(SPACE_LG)
        .rounded(RADIUS_LG)
        .bg(surface_bg(t.bg_header, cx))
        .border_1()
        .border_color(p.border_subtle)
        .child(
            h_flex()
                .flex_shrink_0()
                .gap(SPACE_SM)
                .items_center()
                .pb(SPACE_MD)
                .border_b_1()
                .border_color(p.border_subtle)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .w(px(18.0))
                        .h(px(18.0))
                        .rounded(RADIUS_STD)
                        .bg(surface_bg(t.bg_selection, cx))
                        .child(
                            icon.size(ICON_MICRO)
                                .text_color(p.status_info),
                        ),
                )
                .child(
                    div()
                        .text_size(ui_text_md(cx))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(t.text_secondary))
                        .child(title),
                ),
        )
        .child(div().flex_1().flex().flex_col().gap(SPACE_SM).child(body))
        .into_any_element()
}

impl StatusBar {
    /// Renders the server resource monitor popup anchored above the status bar.
    /// Data comes from the live `SystemInfoCache` (auto-refreshed on a timer),
    /// so CPU / memory / disk / network figures update in place.
    fn render_monitor_panel(
        &mut self,
        t: &ThemeColors,
        stats: &SystemStats,
        ip: &str,
        services: &[ServiceDefinition],
        runtime: &HashMap<String, ServiceRuntimeState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = SemanticPalette::from_context(cx);
        let title = i18n!(cx, "status.monitor.title");
        let close_label = i18n!(cx, "common.action.close");
        let now = format_datetime();

        let (_enable_monitor, monitor_cpu, monitor_mem, monitor_disk) =
            self.focused_monitor_config(cx);

        let cpu_pct = stats.cpu_usage as i32;
        let mem_pct = if stats.memory_total_gb > 0.0 {
            (stats.memory_used_gb / stats.memory_total_gb * 100.0) as i32
        } else {
            0
        };

        let rx_mb = stats.net_rx_rate / 1_048_576.0;
        let tx_mb = stats.net_tx_rate / 1_048_576.0;

        // Top status bar: uptime / load / users (left) + system time (right).
        let status_bar = div()
            .id("monitor-status-bar")
            .flex()
            .items_center()
            .justify_between()
            .px(SPACE_LG)
            .py(SPACE_MD)
            .rounded(px(6.0))
            .bg(surface_bg(t.bg_header, cx))
            .border_1()
            .border_color(p.border_subtle)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(SPACE_XL)
                    .child(monitor_status_item(
                        t,
                        cx,
                        AppIcon::MonitorClock,
                        format_uptime(stats.uptime_secs),
                    ))
                    .child(monitor_status_item(
                        t,
                        cx,
                        AppIcon::Activity,
                        format!(
                            "{:.2}, {:.2}, {:.2}",
                            stats.load_one, stats.load_five, stats.load_fifteen
                        ),
                    ))
                    .child(monitor_status_item(
                        t,
                        cx,
                        AppIcon::MonitorUsers,
                        format!("{}", stats.user_count),
                    )),
            )
            .child(
                div()
                    .text_size(ui_text_md(cx))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(t.text_primary))
                    .child(now),
            );

        // System card body — hostname, OS, kernel, arch (no IP).
        let host_display = if stats.hostname.is_empty() {
            "localhost".to_string()
        } else {
            stats.hostname.clone()
        };
        let system_body = div()
            .flex()
            .flex_col()
            .gap(SPACE_XS)
            .child({
                let host_trigger = div()
                    .id("monitor-hostname")
                    .text_size(ui_text(24.0, cx))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(t.text_primary))
                    .truncate()
                    .overflow_hidden()
                    .child(host_display.clone());
                host_trigger.tooltip(move |_, cx| {
                    cx.new(|_| Tooltip::new(host_display.clone()).direction(TooltipDirection::Top))
                        .into()
                })
            })
            // Spacer line so the OS name + kernel/arch rows below align with the
            // CPU card's brand + cores/processes rows.
            .child(div().h(px(16.0)))
            .child(monitor_icon_text(
                t,
                cx,
                AppIcon::Monitor,
                format!("{} {}", stats.os_name, stats.os_version),
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(monitor_icon_text(
                        t,
                        cx,
                        AppIcon::Monitor,
                        stats.kernel.clone(),
                    ))
                    .child(monitor_icon_text(t, cx, AppIcon::Cpu, stats.arch.clone())),
            );

        // CPU card body — usage above bar, name (no label), divider, cores/processes below.
        let cpu_body = div()
            .flex()
            .flex_col()
            .gap(SPACE_MD)
            .child(
                div()
                    .text_size(ui_text(24.0, cx))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(t.text_primary))
                    .child(format!("{}%", cpu_pct)),
            )
            .child(meter_bar(t, cpu_pct))
            .child(
                div()
                    .text_size(ui_text_sm(cx))
                    .text_color(rgb(t.text_muted))
                    .child(stats.cpu_brand.clone()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(monitor_icon_text(
                        t,
                        cx,
                        AppIcon::Cpu,
                        format!("{} {}", stats.cpu_cores, i18n!(cx, "status.monitor.cores")),
                    ))
                    .child(monitor_icon_text(
                        t,
                        cx,
                        AppIcon::Activity,
                        format!(
                            "{} {}",
                            stats.process_count,
                            i18n!(cx, "status.monitor.processes")
                        ),
                    )),
            );

        // Memory card middle height: header row (used/total + %) above the bar.
        // The top-level `flex_1` below keeps the divider aligned with the network
        // card regardless of this value.
        let mem_middle_h = px(44.0);
        let mem_color = meter_color(t, mem_pct);

        // Memory card body — used/total + bar pinned to `mem_middle_h`. The
        // top-level `flex_1` makes the body fill the (equal-height) card area
        // so the internal spacer can push the divider + available/swap row to
        // the bottom, aligning it with the network card's divider.
        let mem_body = div()
            .flex()
            .flex_1()
            .flex_col()
            .child(
                div()
                    .h(mem_middle_h)
                    .flex()
                    .flex_col()
                    .gap(SPACE_MD)
                    // Header row above the bar: used/total on the left (used
                    // enlarged + bright to stand out, total shrunk), the main
                    // percentage on the right colored by the meter threshold.
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(SPACE_XS)
                                    .child(
                                        div()
                                            .text_size(ui_text(18.0, cx))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(rgb(t.text_primary))
                                            .child(fmt_gb(stats.memory_used_gb)),
                                    )
                                    .child(
                                        div()
                                            .text_size(ui_text_ms(cx))
                                            .text_color(rgb(t.text_muted))
                                            .child(format!(" / {}", fmt_gb(stats.memory_total_gb))),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(ui_text_ms(cx))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(mem_color))
                                    .child(format!("{}%", mem_pct)),
                            ),
                    )
                    .child(meter_bar(t, mem_pct)),
            )
            .child(div().flex_1())
            .child(div().w_full().h(px(1.0)).bg(p.border_subtle))
            .child(
                div()
                    .flex()
                    .pt(SPACE_SM)
                    .items_center()
                    .justify_between()
                    .child(monitor_icon_text(
                        t,
                        cx,
                        AppIcon::Check,
                        format!(
                            "{} {}",
                            fmt_gb(stats.memory_total_gb - stats.memory_used_gb),
                            i18n!(cx, "status.monitor.available")
                        ),
                    ))
                    .child(monitor_icon_text(
                        t,
                        cx,
                        AppIcon::Memory,
                        format!(
                            "{} {:.1} GB",
                            i18n!(cx, "status.monitor.swap"),
                            stats.swap_total_gb
                        ),
                    )),
            );

        // Network card body — the two tinted speed rows render at their natural
        // height (they must NOT be clamped, otherwise the download text would
        // overflow and overlap the divider). A bottom padding keeps the upload
        // row's tinted background clear of the divider. The top-level `flex_1`
        // makes the body fill the (equal-height) card area, so the internal
        // spacer pushes the divider + IP row to the bottom, aligning it with
        // the memory card.
        let net_body = div()
            .flex()
            .flex_1()
            .flex_col()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(SPACE_SM)
                    .pb(SPACE_MD)
                    .child(monitor_speed_item(
                        t,
                        cx,
                        AppIcon::Download,
                        i18n!(cx, "status.monitor.download"),
                        format!("{:.1} MB/s", rx_mb),
                    ))
                    .child(monitor_speed_item(
                        t,
                        cx,
                        AppIcon::Upload,
                        i18n!(cx, "status.monitor.upload"),
                        format!("{:.1} MB/s", tx_mb),
                    )),
            )
            .child(div().flex_1())
            .child(div().w_full().h(px(1.0)).bg(p.border_subtle))
            .child(div().pt(SPACE_SM).child(monitor_icon_text(
                t,
                cx,
                AppIcon::Globe,
                ip.to_string(),
            )));

        // Disk section — table with colored usage badges.
        let disk_section = div()
            .flex()
            .flex_col()
            .gap(SPACE_MD)
            .child(
                h_flex()
                    .justify_center()
                    .gap(SPACE_SM)
                    .items_center()
                    .text_size(ui_text_md(cx))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(t.text_secondary))
                    .child(
                        AppIcon::HardDrive
                            .size(ICON_STD)
                            .text_color(p.status_info),
                    )
                    .child(div().child(i18n!(cx, "status.monitor.disk"))),
            )
            .child(self.render_disk_table(t, cx, &stats.disks));

        let viewport = window.viewport_size();
        let b = *self.monitor_bounds.borrow();
        let sb_b = *self.status_bar_bounds.borrow();
        let pos = Self::calc_monitor_anchor(viewport, b, sb_b, cx);

        let (max_w, max_h) = Self::calc_monitor_max_bounds(
            viewport, pos.x, pos.y, window, cx,
        );

        let settings = settings_entity(cx).read(cx).settings.clone();
        let cur_w = self
            .monitor_panel_width
            .unwrap_or(settings.monitor_popup_width)
            .clamp(MONITOR_PANEL_MIN_W, max_w);
        let cur_h = self
            .monitor_panel_height
            .unwrap_or(settings.monitor_popup_height)
            .clamp(MONITOR_PANEL_MIN_H, max_h);

        let top_handle = div()
            .id("monitor-resize-top")
            .absolute()
            .top(px(0.0))
            .left(px(16.0))
            .right(px(0.0))
            .h(px(8.0))
            .cursor(CursorStyle::ResizeUpDown)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, ev: &MouseDownEvent, _w, cx| {
                    cx.stop_propagation();
                    this.monitor_panel_drag = Some(MonitorPanelDrag {
                        handle: MonitorResizeHandle::Top,
                        start_pos: ev.position,
                        start_w: cur_w,
                        start_h: cur_h,
                    });
                    cx.notify();
                }),
            );

        let left_handle = div()
            .id("monitor-resize-left")
            .absolute()
            .top(px(16.0))
            .left(px(0.0))
            .bottom(px(0.0))
            .w(px(8.0))
            .cursor(CursorStyle::ResizeLeftRight)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, ev: &MouseDownEvent, _w, cx| {
                    cx.stop_propagation();
                    this.monitor_panel_drag = Some(MonitorPanelDrag {
                        handle: MonitorResizeHandle::Left,
                        start_pos: ev.position,
                        start_w: cur_w,
                        start_h: cur_h,
                    });
                    cx.notify();
                }),
            );

        let top_left_handle = div()
            .id("monitor-resize-top-left")
            .absolute()
            .top(px(0.0))
            .left(px(0.0))
            .size(px(16.0))
            .cursor(CursorStyle::ResizeUpLeftDownRight)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, ev: &MouseDownEvent, _w, cx| {
                    cx.stop_propagation();
                    this.monitor_panel_drag = Some(MonitorPanelDrag {
                        handle: MonitorResizeHandle::TopLeft,
                        start_pos: ev.position,
                        start_w: cur_w,
                        start_h: cur_h,
                    });
                    cx.notify();
                }),
            );

        let panel = div()
            .id("monitor-panel")
            .occlude()
            .on_mouse_down_out(cx.listener(|this, _ev, _w, cx| {
                if this.monitor_panel_drag.is_some() {
                    return;
                }
                if this.monitor_suppress_close {
                    this.monitor_suppress_close = false;
                    return;
                }
                if this.monitor_open {
                    this.monitor_open = false;
                    this.monitor_suppress_close = false;
                    cx.notify();
                }
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .on_mouse_move(cx.listener(move |this, ev: &MouseMoveEvent, _w, cx| {
                if let Some(drag) = this.monitor_panel_drag {
                    let delta_x = f32::from(ev.position.x - drag.start_pos.x);
                    let delta_y = f32::from(ev.position.y - drag.start_pos.y);

                    let new_w = match drag.handle {
                        MonitorResizeHandle::Left | MonitorResizeHandle::TopLeft => {
                            (drag.start_w - delta_x).clamp(MONITOR_PANEL_MIN_W, max_w)
                        }
                        _ => drag.start_w,
                    };

                    let new_h = match drag.handle {
                        MonitorResizeHandle::Top | MonitorResizeHandle::TopLeft => {
                            (drag.start_h - delta_y).clamp(MONITOR_PANEL_MIN_H, max_h)
                        }
                        _ => drag.start_h,
                    };

                    let changed = this.monitor_panel_width != Some(new_w)
                        || this.monitor_panel_height != Some(new_h);
                    if changed {
                        this.monitor_panel_width = Some(new_w);
                        this.monitor_panel_height = Some(new_h);
                        cx.notify();
                    }
                }
                if let Some(drag) = this.disk_col_drag {
                    let delta = f32::from(ev.position.x - drag.start_x);
                    let new_w = (drag.start_w + delta).clamp(DISK_COL_MIN, DISK_COL_MAX);
                    if let Some(w) = this.disk_col_widths.get_mut(drag.col) {
                        if (*w - new_w).abs() > f32::EPSILON {
                            *w = new_w;
                            cx.notify();
                        }
                    }
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _ev: &MouseUpEvent, _w, cx| {
                    if let Some(_drag) = this.monitor_panel_drag.take() {
                        if let (Some(w), Some(h)) =
                            (this.monitor_panel_width, this.monitor_panel_height)
                        {
                            settings_entity(cx).update(cx, |s, cx| {
                                s.set_monitor_popup_size(w, h, cx);
                            });
                        }
                        cx.notify();
                    }
                    if this.disk_col_drag.is_some() {
                        this.disk_col_drag = None;
                        cx.notify();
                    }
                }),
            )
            .w(px(cur_w))
            .h(px(cur_h))
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(surface_bg(t.bg_secondary, cx))
            .border_1()
            .border_color(p.border_subtle)
            .rounded(RADIUS_LG)
            .shadow_xl()
            .child(top_handle)
            .child(left_handle)
            .child(top_left_handle)
            // Header (fixed at top, not scrolled)
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(SPACE_LG)
                    .py(SPACE_MD)
                    .border_b_1()
                    .border_color(p.border_subtle)
                    .child(
                        h_flex()
                            .gap(SPACE_SM)
                            .items_center()
                            .child(
                                AppIcon::SquareActivity
                                    .size(ICON_MD)
                                    .text_color(p.status_info),
                            )
                            .child(
                                div()
                                    .text_size(ui_text_md(cx))
                                    .text_color(rgb(t.text_primary))
                                    .child(title),
                            ),
                    )
                    .child({
                        let close_trigger =
                            icon_button("monitor-panel-close", AppIcon::Close, &t, cx).on_click(
                                cx.listener(|this, _ev, _w, cx| {
                                    this.monitor_open = false;
                                    cx.notify();
                                }),
                            );
                        close_trigger.tooltip(move |_, cx| {
                            cx.new(|_| {
                                Tooltip::new(close_label.clone()).direction(TooltipDirection::Top)
                            })
                            .into()
                        })
                    }),
            )
            // Tab bar (fixed below header, not scrolled)
            .child(div().flex_shrink_0().child(self.monitor_tab_bar(t, cx)))
            // Scrollable Body area (with vertical Scrollbar overlay)
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(if self.monitor_tab == MonitorTab::Service {
                        v_flex()
                            .id("monitor-service-body")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.service_scroll_handle)
                            .p(SPACE_LG)
                            .child(self.render_service_table(t, cx, services, runtime, cur_w))
                    } else {
                        v_flex()
                            .id("monitor-panel-body")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.monitor_scroll_handle)
                            .p(SPACE_LG)
                            .gap(SPACE_LG)
                            .child(status_bar)
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(SPACE_LG)
                                    .child(
                                        div()
                                            .flex()
                                            .gap(SPACE_LG)
                                            .child(monitor_card(
                                                t,
                                                cx,
                                                i18n!(cx, "status.monitor.system"),
                                                AppIcon::Monitor,
                                                system_body.into_any_element(),
                                            ))
                                            .when(monitor_cpu, |el| {
                                                el.child(monitor_card(
                                                    t,
                                                    cx,
                                                    i18n!(cx, "status.monitor.cpu"),
                                                    AppIcon::Cpu,
                                                    cpu_body.into_any_element(),
                                                ))
                                            }),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .gap(SPACE_LG)
                                            .when(monitor_mem, |el| {
                                                el.child(monitor_card(
                                                    t,
                                                    cx,
                                                    i18n!(cx, "status.monitor.mem"),
                                                    AppIcon::Memory,
                                                    mem_body.into_any_element(),
                                                ))
                                            })
                                            .child(monitor_card(
                                                t,
                                                cx,
                                                i18n!(cx, "status.monitor.network"),
                                                AppIcon::Network,
                                                net_body.into_any_element(),
                                            )),
                                    ),
                            )
                            .when(monitor_disk, |el| el.child(disk_section))
                    })
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .right_0()
                            .left_0()
                            .child(if self.monitor_tab == MonitorTab::Service {
                                Scrollbar::vertical(&self.service_scroll_handle)
                            } else {
                                Scrollbar::vertical(&self.monitor_scroll_handle)
                            }),
                    ),
            );

        panel.into_any_element()
    }

    /// Tab bar at the top of the merged monitor popup, switching between the
    /// Resources and Services views.
    fn monitor_tab_bar(&self, t: &ThemeColors, cx: &Context<Self>) -> impl IntoElement {
        let p = SemanticPalette::from_context(cx);
        let tabs: [(MonitorTab, String, AppIcon); 2] = [
            (
                MonitorTab::Resource,
                i18n!(cx, "status.monitor.tab_resource"),
                AppIcon::Server,
            ),
            (
                MonitorTab::Service,
                i18n!(cx, "status.monitor.tab_service"),
                AppIcon::SquareActivity,
            ),
        ];
        h_flex()
            .id("monitor-tab-bar")
            .gap(SPACE_2XS)
            .px(SPACE_MD)
            .py(SPACE_XS)
            .border_b_1()
            .border_color(p.border_subtle)
            .children(tabs.into_iter().map(|(tab, label, icon)| {
                let active = self.monitor_tab == tab;
                let id = match tab {
                    MonitorTab::Resource => "monitor-tab-resource",
                    MonitorTab::Service => "monitor-tab-service",
                };
                h_flex()
                    .id(id)
                    .cursor_pointer()
                    .gap(SPACE_SM)
                    .items_center()
                    .px(SPACE_MD)
                    .py(SPACE_XS)
                    .rounded(RADIUS_STD)
                    .when(active, |el| el.bg(surface_bg(t.bg_selection, cx)))
                    .child(icon.size(ICON_STD).text_color(if active {
                        rgb(t.accent)
                    } else {
                        rgb(t.text_muted)
                    }))
                    .child(
                        div()
                            .text_size(ui_text_md(cx))
                            .text_color(if active {
                                rgb(t.text_primary)
                            } else {
                                rgb(t.text_muted)
                            })
                            .child(label),
                    )
                    .on_click(cx.listener(move |this, _ev, _w, cx| {
                        this.monitor_tab = tab;
                        cx.notify();
                    }))
                    .into_any_element()
            }))
    }


    /// Execute a start/stop/restart operation on a service.
    fn execute_service_op(
        &mut self,
        service: &ServiceDefinition,
        op: ServiceOp,
        cx: &mut Context<Self>,
    ) {
        let cmd = match op {
            ServiceOp::Start => service.effective_start_command(),
            ServiceOp::Stop => service.effective_stop_command(),
            ServiceOp::Restart => service.effective_restart_command(),
        };
        let Some(cmd) = cmd else {
            return;
        };

        let svc_name = service.name.clone();
        let svc_id = service.id.clone();
        let op_label = match op {
            ServiceOp::Start => i18n!(cx, "service.start"),
            ServiceOp::Stop => i18n!(cx, "service.stop"),
            ServiceOp::Restart => i18n!(cx, "service.restart"),
        };

        match &service.session_id {
            Some(_) => {
                let engine = cx
                    .try_global::<GlobalServiceMonitorEngine>()
                    .map(|e| e.0.clone());
                if let Some(engine) = engine {
                    let snapshot = engine.read(cx).snapshot_for_probe();
                    if let Some(handle) = snapshot.2 {
                        self.service_operating_ids.insert(svc_id.clone(), op);
                        cx.notify();

                        let def = service.clone();
                        let op_label = op_label.clone();
                        let svc_name = svc_name.clone();
                        let engine_clone = engine.clone();
                        let tracked_id = svc_id.clone();
                        cx.spawn(async move |this: WeakEntity<StatusBar>, cx| {
                            let res = cx
                                .background_executor()
                                .spawn(async move {
                                    ServiceMonitorEngine::exec_command(&def, op, &handle)
                                })
                                .await;

                            let _ = this.update(cx, |this, cx| {
                                this.service_operating_ids.remove(&tracked_id);
                                match res {
                                    Ok(_) => {
                                        ToastManager::success(
                                            format!("{}「{}」成功", op_label, svc_name),
                                            cx,
                                        );
                                    }
                                    Err(e) => {
                                        ToastManager::error(
                                            format!("{}「{}」失败: {}", op_label, svc_name, e),
                                            cx,
                                        );
                                    }
                                }
                                cx.notify();
                            });

                            let snapshot = this
                                .update(cx, |_this, cx| {
                                    engine_clone.read(cx).snapshot_for_probe()
                                })
                                .ok();
                            if let Some((Some(_), services, Some(handle))) = snapshot {
                                let results = cx
                                    .background_executor()
                                    .spawn(async move {
                                        ServiceMonitorEngine::probe(&services, &handle)
                                    })
                                    .await;
                                let _ = this.update(cx, |_this, cx| {
                                    engine_clone.update(cx, |e, cx| {
                                        e.apply_results(results, cx);
                                    });
                                });
                            }
                        })
                        .detach();
                    }
                }
            }
            None => {
                let focus_manager = self.focus_manager.clone();
                let workspace = self.workspace.clone();
                let terminals = self.terminals.clone();
                send_command_to_focused_terminal(
                    &focus_manager,
                    &workspace,
                    &terminals,
                    &cmd,
                    cx,
                );
                ToastManager::info(format!("已发送{}命令至终端", op_label), cx);
            }
        }
    }

    /// Trigger a full probe for all monitored services in the active session.
    pub fn trigger_service_probe_all(&mut self, cx: &mut Context<Self>) {
        if self.service_is_refreshing_all {
            return;
        }
        self.service_is_refreshing_all = true;
        cx.notify();

        let engine = match cx.try_global::<GlobalServiceMonitorEngine>() {
            Some(e) => e.0.clone(),
            None => {
                self.service_is_refreshing_all = false;
                return;
            }
        };

        self.sync_service_monitor_target(cx);
        let snapshot = engine.read(cx).snapshot_for_probe();
        let (_session_id, services, handle) = snapshot;

        let Some(handle) = handle else {
            self.service_is_refreshing_all = false;
            cx.notify();
            return;
        };

        if services.is_empty() {
            self.service_is_refreshing_all = false;
            cx.notify();
            return;
        }

        cx.spawn(async move |this: WeakEntity<StatusBar>, cx| {
            let results = cx
                .background_executor()
                .spawn(async move { ServiceMonitorEngine::probe(&services, &handle) })
                .await;

            let _ = this.update(cx, |this, cx| {
                this.service_is_refreshing_all = false;
                engine.update(cx, |e, cx| {
                    e.apply_results(results, cx);
                });
                cx.notify();
            });
        })
        .detach();
    }

    /// Trigger an individual probe for a single service.
    pub fn trigger_service_probe_single(&mut self, service_id: &str, cx: &mut Context<Self>) {
        let sid = service_id.to_string();
        if self.service_probing_ids.contains(&sid) {
            return;
        }
        self.service_probing_ids.insert(sid.clone());
        cx.notify();

        let engine = match cx.try_global::<GlobalServiceMonitorEngine>() {
            Some(e) => e.0.clone(),
            None => {
                self.service_probing_ids.remove(&sid);
                return;
            }
        };

        self.sync_service_monitor_target(cx);
        let snapshot = engine.read(cx).snapshot_for_probe();
        let (_session_id, services, handle) = snapshot;

        let Some(handle) = handle else {
            self.service_probing_ids.remove(&sid);
            cx.notify();
            return;
        };

        let target_service: Vec<ServiceDefinition> = services
            .into_iter()
            .filter(|s| s.id == sid)
            .collect();

        if target_service.is_empty() {
            self.service_probing_ids.remove(&sid);
            cx.notify();
            return;
        }

        cx.spawn(async move |this: WeakEntity<StatusBar>, cx| {
            let results = cx
                .background_executor()
                .spawn(async move { ServiceMonitorEngine::probe(&target_service, &handle) })
                .await;

            let _ = this.update(cx, |this, cx| {
                this.service_probing_ids.remove(&sid);
                engine.update(cx, |e, cx| {
                    e.apply_results(results, cx);
                });
                cx.notify();
            });
        })
        .detach();
    }

    /// Render a single modern service card.
    fn render_service_card(
        &self,
        service: &ServiceDefinition,
        status: ServiceStatus,
        is_probing: bool,
        t: &ThemeColors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = SemanticPalette::from_context(cx);
        let is_running = status == ServiceStatus::Running;

        // Status badge with LED dot
        let (dot_color, badge_bg, status_text) = match status {
            ServiceStatus::Running => (
                p.status_success,
                p.status_success.opacity(0.12),
                i18n!(cx, "common.status.running"),
            ),
            ServiceStatus::Stopped => (
                p.status_error,
                p.status_error.opacity(0.10),
                i18n!(cx, "common.status.stopped"),
            ),
            ServiceStatus::Starting => (
                p.status_warning,
                p.status_warning.opacity(0.10),
                i18n!(cx, "service.status.starting"),
            ),
            ServiceStatus::Stopping => (
                p.status_warning,
                p.status_warning.opacity(0.10),
                i18n!(cx, "service.status.stopping"),
            ),
            _ => (
                p.text_muted,
                p.surface_card,
                i18n!(cx, "service.status.not_checked"),
            ),
        };

        // Row 1: Icon + Service Name (Full-width row, auto-truncated with ellipsis)
        let name_row = h_flex()
            .w_full()
            .items_center()
            .gap(SPACE_SM)
            .min_w_0()
            .child(
                AppIcon::SquareActivity
                    .size(ICON_STD)
                    .text_color(if is_running { p.status_success } else { p.text_muted }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_size(ui_text_md(cx))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(t.text_primary))
                    .child(service.name.clone()),
            );

        // Row 2: Status Chip (Full-width row, standalone on its own line)
        let status_row = h_flex()
            .w_full()
            .items_center()
            .child(
                h_flex()
                    .gap(SPACE_XS)
                    .items_center()
                    .px(SPACE_SM)
                    .py(px(2.0))
                    .rounded(RADIUS_MD)
                    .bg(badge_bg)
                    .child(
                        div()
                            .size(px(6.0))
                            .rounded_full()
                            .bg(dot_color),
                    )
                    .child(
                        div()
                            .text_size(ui_text_sm(cx))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(dot_color)
                            .child(status_text),
                    ),
            );

        // Row 3: Action buttons (Centered, fixed equal-width buttons, always visible with state-based disabled styling)
        let s_stop = service.clone();
        let s_start = service.clone();
        let s_restart = service.clone();
        let s_refresh_id = service.id.clone();
        let refresh_tip = i18n!(cx, "service.refresh_single");

        let current_op = self.service_operating_ids.get(&service.id).copied();
        let is_operating = current_op.is_some();
        let is_any_busy = is_operating || is_probing;

        // 1) Start button
        let start_btn = {
            let is_start_loading = current_op == Some(ServiceOp::Start);
            let base = h_flex()
                .id(format!("card-btn-start-{}", service.id))
                .flex_1()
                .min_w(px(46.0))
                .items_center()
                .justify_center()
                .gap(SPACE_2XS)
                .py(px(4.0))
                .px(SPACE_2XS)
                .rounded(RADIUS_STD)
                .text_size(ui_text_sm(cx));

            if is_start_loading {
                let anim_id = format!("card-spin-start-{}", service.id);
                let spinner = velowork_ui::spinner::loading_spinner(anim_id, ICON_SM, p.status_success);
                base.border_1()
                    .border_color(p.status_success)
                    .bg(p.status_success.opacity(0.16))
                    .text_color(p.status_success)
                    .font_weight(FontWeight::SEMIBOLD)
                    .opacity(0.50)
                    .cursor(CursorStyle::Arrow)
                    .child(spinner)
                    .child(i18n!(cx, "service.start"))
            } else if !is_running && !is_any_busy {
                base.border_1()
                    .border_color(p.status_success)
                    .bg(p.status_success.opacity(0.16))
                    .text_color(p.status_success)
                    .font_weight(FontWeight::SEMIBOLD)
                    .cursor_pointer()
                    .hover(|s| s.bg(p.status_success.opacity(0.26)))
                    .on_click(cx.listener(move |this, _, _w, cx| {
                        this.execute_service_op(&s_start, ServiceOp::Start, cx);
                    }))
                    .child(i18n!(cx, "service.start"))
            } else {
                base.border_1()
                    .border_color(p.border_subtle)
                    .bg(surface_bg(t.bg_secondary, cx))
                    .text_color(p.text_muted)
                    .font_weight(FontWeight::NORMAL)
                    .opacity(0.35)
                    .cursor(CursorStyle::Arrow)
                    .child(i18n!(cx, "service.start"))
            }
        };

        // 2) Stop button
        let stop_btn = {
            let is_stop_loading = current_op == Some(ServiceOp::Stop);
            let base = h_flex()
                .id(format!("card-btn-stop-{}", service.id))
                .flex_1()
                .min_w(px(46.0))
                .items_center()
                .justify_center()
                .gap(SPACE_2XS)
                .py(px(4.0))
                .px(SPACE_2XS)
                .rounded(RADIUS_STD)
                .text_size(ui_text_sm(cx));

            if is_stop_loading {
                let anim_id = format!("card-spin-stop-{}", service.id);
                let spinner = velowork_ui::spinner::loading_spinner(anim_id, ICON_SM, p.text_secondary);
                base.border_1()
                    .border_color(p.border_subtle)
                    .bg(surface_bg(t.bg_secondary, cx))
                    .text_color(p.text_secondary)
                    .font_weight(FontWeight::MEDIUM)
                    .opacity(0.50)
                    .cursor(CursorStyle::Arrow)
                    .child(spinner)
                    .child(i18n!(cx, "service.stop"))
            } else if is_running && !is_any_busy {
                base.border_1()
                    .border_color(p.border_subtle)
                    .bg(surface_bg(t.bg_secondary, cx))
                    .text_color(p.text_secondary)
                    .font_weight(FontWeight::MEDIUM)
                    .cursor_pointer()
                    .hover(|s| s.bg(surface_bg(t.bg_hover, cx)).border_color(p.border_active))
                    .on_click(cx.listener(move |this, _, _w, cx| {
                        this.execute_service_op(&s_stop, ServiceOp::Stop, cx);
                    }))
                    .child(i18n!(cx, "service.stop"))
            } else {
                base.border_1()
                    .border_color(p.border_subtle)
                    .bg(surface_bg(t.bg_secondary, cx))
                    .text_color(p.text_muted)
                    .font_weight(FontWeight::NORMAL)
                    .opacity(0.35)
                    .cursor(CursorStyle::Arrow)
                    .child(i18n!(cx, "service.stop"))
            }
        };

        // 3) Restart button
        let restart_btn = {
            let is_restart_loading = current_op == Some(ServiceOp::Restart);
            let base = h_flex()
                .id(format!("card-btn-restart-{}", service.id))
                .flex_1()
                .min_w(px(46.0))
                .items_center()
                .justify_center()
                .gap(SPACE_2XS)
                .py(px(4.0))
                .px(SPACE_2XS)
                .rounded(RADIUS_STD)
                .text_size(ui_text_sm(cx));

            if is_restart_loading {
                let anim_id = format!("card-spin-restart-{}", service.id);
                let spinner = velowork_ui::spinner::loading_spinner(anim_id, ICON_SM, p.text_secondary);
                base.border_1()
                    .border_color(p.border_subtle)
                    .bg(surface_bg(t.bg_secondary, cx))
                    .text_color(p.text_secondary)
                    .font_weight(FontWeight::MEDIUM)
                    .opacity(0.50)
                    .cursor(CursorStyle::Arrow)
                    .child(spinner)
                    .child(i18n!(cx, "service.restart"))
            } else if !is_any_busy {
                base.border_1()
                    .border_color(p.border_subtle)
                    .bg(surface_bg(t.bg_secondary, cx))
                    .text_color(p.text_secondary)
                    .font_weight(FontWeight::MEDIUM)
                    .cursor_pointer()
                    .hover(|s| s.bg(surface_bg(t.bg_hover, cx)).border_color(p.border_active))
                    .on_click(cx.listener(move |this, _, _w, cx| {
                        this.execute_service_op(&s_restart, ServiceOp::Restart, cx);
                    }))
                    .child(i18n!(cx, "service.restart"))
            } else {
                base.border_1()
                    .border_color(p.border_subtle)
                    .bg(surface_bg(t.bg_secondary, cx))
                    .text_color(p.text_muted)
                    .font_weight(FontWeight::NORMAL)
                    .opacity(0.35)
                    .cursor(CursorStyle::Arrow)
                    .child(i18n!(cx, "service.restart"))
            }
        };

        // 4) Refresh button
        let refresh_btn = {
            let base = h_flex()
                .id(format!("card-btn-refresh-{}", service.id))
                .flex_1()
                .min_w(px(46.0))
                .items_center()
                .justify_center()
                .gap(SPACE_2XS)
                .py(px(4.0))
                .px(SPACE_2XS)
                .rounded(RADIUS_STD)
                .border_1()
                .border_color(p.border_subtle)
                .bg(surface_bg(t.bg_secondary, cx))
                .text_color(p.text_secondary)
                .text_size(ui_text_sm(cx))
                .font_weight(FontWeight::MEDIUM)
                .tooltip(move |_, cx| {
                    cx.new(|_| Tooltip::new(refresh_tip.clone()).direction(TooltipDirection::Top))
                        .into()
                });

            if is_probing {
                let anim_id = format!("card-spin-refresh-{}", service.id);
                let spinner = velowork_ui::spinner::loading_spinner(anim_id, ICON_SM, p.text_secondary);
                base.opacity(0.50)
                    .cursor(CursorStyle::Arrow)
                    .child(spinner)
                    .child(i18n!(cx, "common.action.refresh"))
            } else if is_operating {
                base.opacity(0.35)
                    .cursor(CursorStyle::Arrow)
                    .child(i18n!(cx, "common.action.refresh"))
            } else {
                base.cursor_pointer()
                    .hover(|s| s.bg(surface_bg(t.bg_hover, cx)).border_color(p.border_active))
                    .on_click(cx.listener(move |this, _, _w, cx| {
                        this.trigger_service_probe_single(&s_refresh_id, cx);
                    }))
                    .child(i18n!(cx, "common.action.refresh"))
            }
        };

        let actions_row = h_flex()
            .w_full()
            .gap(SPACE_XS)
            .items_center()
            .justify_center()
            .child(start_btn)
            .child(stop_btn)
            .child(restart_btn)
            .child(refresh_btn);

        // Context menu on right click
        let def_menu = service.clone();
        let status_menu = status;
        let terms = self.terminals.clone();
        let om = self.overlay_manager.clone();

        v_flex()
            .id(format!("service-card-{}", service.id))
            .w_full()
            .p(SPACE_LG)
            .gap(px(14.0))
            .rounded(RADIUS_LG)
            .bg(surface_bg(t.bg_panel, cx))
            .border_1()
            .border_color(p.border_subtle)
            .hover(|s| s.border_color(p.border_active).bg(surface_bg(t.bg_hover, cx)))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |_this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    let req = ServiceMenuRequest {
                        position: event.position,
                        target: ServiceMenuTarget::Node(ServiceNode::Service {
                            def: def_menu.clone(),
                        }),
                        selected_ids: vec![def_menu.id.clone()],
                        status: Some(status_menu),
                        ops_only: true,
                    };
                    om.update(cx, |om, cx| {
                        om.show_service_context_menu(req, terms.clone(), window, cx);
                    });
                }),
            )
            .child(name_row)
            .child(status_row)
            .child(actions_row)
            .into_any_element()
    }

    /// Service card list shown under the Services tab with statistics, search,
    /// filtering, and a responsive grid layout.
    fn render_service_table(
        &mut self,
        t: &ThemeColors,
        cx: &mut Context<Self>,
        services: &[ServiceDefinition],
        runtime: &HashMap<String, ServiceRuntimeState>,
        cur_w: f32,
    ) -> AnyElement {
        let p = SemanticPalette::from_context(cx);

        let status_of = |id: &str| -> ServiceStatus {
            runtime
                .get(id)
                .map(|r| r.status)
                .unwrap_or(ServiceStatus::NotChecked)
        };
        let total = services.len();
        let running = services
            .iter()
            .filter(|s| status_of(&s.id) == ServiceStatus::Running)
            .count();
        let stopped = services
            .iter()
            .filter(|s| status_of(&s.id) != ServiceStatus::Running)
            .count();

        if services.is_empty() {
            let active_session_id = self.focused_ssh_info(cx).0;
            let btn_tip = i18n!(cx, "service.go_to_add_service");
            return v_flex()
                .w_full()
                .px(SPACE_LG)
                .py(SPACE_XL)
                .gap(SPACE_MD)
                .items_center()
                .justify_center()
                .child(
                    AppIcon::SquareActivity
                        .size(ICON_LG)
                        .text_color(p.text_muted),
                )
                .child(
                    div()
                        .child(i18n!(cx, "service.no_services"))
                        .text_color(p.text_primary)
                        .text_size(ui_text_md(cx))
                        .font_weight(FontWeight::SEMIBOLD)
                        .whitespace_nowrap(),
                )
                .child(
                    div()
                        .child(i18n!(cx, "service.no_services_hint"))
                        .text_color(p.text_muted)
                        .text_size(ui_text_md(cx))
                        .whitespace_nowrap(),
                )
                .child(
                    h_flex()
                        .id("btn-go-to-add-service")
                        .mt(SPACE_XS)
                        .gap(SPACE_SM)
                        .items_center()
                        .px(SPACE_LG)
                        .py(SPACE_SM)
                        .rounded(RADIUS_STD)
                        .bg(surface_bg(t.bg_selection, cx))
                        .border_1()
                        .border_color(p.border_subtle)
                        .cursor_pointer()
                        .hover(|s| {
                            s.bg(surface_bg(t.bg_hover, cx))
                                .border_color(p.border_active)
                        })
                        .tooltip(move |_, cx| {
                            cx.new(|_| {
                                Tooltip::new(btn_tip.clone()).direction(TooltipDirection::Top)
                            })
                            .into()
                        })
                        .child(AppIcon::Plus.size(ICON_STD).text_color(rgb(t.accent)))
                        .child(
                            div()
                                .text_size(ui_text_md(cx))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(rgb(t.text_primary))
                                .whitespace_nowrap()
                                .child(i18n!(cx, "service.go_to_add_service")),
                        )
                        .on_click(cx.listener(move |this, _, _window, cx| {
                            this.monitor_open = false;
                            if let Some(dock) = &this.right_dock {
                                dock.update(cx, |d, cx| {
                                    d.select_tab_by_id("services", cx);
                                });
                            }
                            let mut initial = ServiceDefinition::default();
                            initial.session_id = active_session_id.clone();
                            initial.monitor_enabled = true;
                            this.overlay_manager.update(cx, |om, cx| {
                                om.show_service_dialog(
                                    ServiceDialogMode::Create { parent_id: None },
                                    Some(initial),
                                    cx,
                                );
                            });
                        })),
                )
                .into_any_element();
        }

        // 1. Stat cards: 【全部】(Total), 【运行中】(Running), 【已停止】(Stopped)
        let stat_card = |label: String, count: usize, color: Hsla, filter: ServiceFilterState| {
            v_flex()
                .id(format!("stat-card-{:?}", filter))
                .flex_1()
                .min_w_0()
                .items_center()
                .justify_center()
                .py(SPACE_SM)
                .px(SPACE_MD)
                .rounded(RADIUS_LG)
                .bg(surface_bg(t.bg_secondary, cx))
                .border_1()
                .border_color(p.border_subtle)
                .cursor_pointer()
                .hover(|s| s.bg(surface_bg(t.bg_hover, cx)).border_color(p.border_active))
                .on_click(cx.listener(move |this, _, _w, cx| {
                    this.service_filter = filter;
                    cx.notify();
                }))
                .child(
                    div()
                        .text_size(px(22.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(color)
                        .line_height(relative(1.1))
                        .child(count.to_string()),
                )
                .child(
                    div()
                        .text_size(ui_text_sm(cx))
                        .text_color(p.text_muted)
                        .mt(px(2.0))
                        .child(label),
                )
        };

        let stats_row = h_flex()
            .w_full()
            .gap(SPACE_MD)
            .child(stat_card(
                i18n!(cx, "service.stat_all"),
                total,
                rgb(t.accent).into(),
                ServiceFilterState::All,
            ))
            .child(stat_card(
                i18n!(cx, "common.status.running"),
                running,
                p.status_success,
                ServiceFilterState::Running,
            ))
            .child(stat_card(
                i18n!(cx, "common.status.stopped"),
                stopped,
                p.status_error,
                ServiceFilterState::Stopped,
            ));

        // 2. Toolbar: Search input + Filter pills + Refresh button
        let is_refreshing_all = self.service_is_refreshing_all;
        let refresh_tip = i18n!(cx, "service.refresh_all");
        let active_filter = self.service_filter;

        let filter_pill = |label: String, filter: ServiceFilterState| {
            let is_active = active_filter == filter;
            div()
                .id(format!("filter-pill-{:?}", filter))
                .cursor_pointer()
                .px(SPACE_MD)
                .py(SPACE_2XS)
                .rounded(RADIUS_STD)
                .text_size(ui_text_sm(cx))
                .font_weight(if is_active { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
                .text_color(if is_active { p.text_primary } else { p.text_muted })
                .bg(if is_active { surface_bg(t.bg_selection, cx) } else { surface_bg(t.bg_secondary, cx) })
                .border_1()
                .border_color(if is_active { p.border_active } else { p.border_subtle })
                .hover(|s| s.bg(surface_bg(t.bg_hover, cx)).border_color(p.border_active))
                .on_click(cx.listener(move |this, _, _w, cx| {
                    this.service_filter = filter;
                    cx.notify();
                }))
                .child(label)
        };

        let toolbar = h_flex()
            .w_full()
            .items_center()
            .justify_between()
            .gap(SPACE_MD)
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap(SPACE_MD)
                    .child(
                        div()
                            .w(px(200.0))
                            .max_w(px(260.0))
                            .min_w(px(120.0))
                            .child(Input::new(&self.service_search_input).search(true)),
                    )
                    .child(
                        h_flex()
                            .gap(SPACE_XS)
                            .items_center()
                            .child(filter_pill(i18n!(cx, "service.filter_all"), ServiceFilterState::All))
                            .child(filter_pill(i18n!(cx, "common.status.running"), ServiceFilterState::Running))
                            .child(filter_pill(i18n!(cx, "common.status.stopped"), ServiceFilterState::Stopped)),
                    ),
            )
            .child(
                velowork_ui::icon_button::icon_button_loading("svc-refresh-all-btn", AppIcon::Refresh, is_refreshing_all, t, cx)
                    .when(!is_refreshing_all, |b| {
                        b.on_click(cx.listener(|this, _, _w, cx| {
                            this.trigger_service_probe_all(cx);
                        }))
                    })
                    .tooltip(move |_, cx| {
                        cx.new(|_| Tooltip::new(refresh_tip.clone()).direction(TooltipDirection::Top))
                            .into()
                    }),
            );

        // 3. Filter services
        let query = self.service_search_input.read(cx).value().trim().to_lowercase();
        let filtered: Vec<&ServiceDefinition> = services
            .iter()
            .filter(|s| {
                let status = status_of(&s.id);
                let matches_filter = match self.service_filter {
                    ServiceFilterState::All => true,
                    ServiceFilterState::Running => status == ServiceStatus::Running,
                    ServiceFilterState::Stopped => status != ServiceStatus::Running,
                };
                if !matches_filter {
                    return false;
                }
                if query.is_empty() {
                    return true;
                }
                s.name.to_lowercase().contains(&query)
                    || s.alive_command.to_lowercase().contains(&query)
            })
            .collect();

        // 4. Grid of cards
        let content: AnyElement = if filtered.is_empty() {
            v_flex()
                .w_full()
                .py(SPACE_XL)
                .items_center()
                .justify_center()
                .gap(SPACE_SM)
                .child(
                    AppIcon::Search
                        .size(ICON_LG)
                        .text_color(p.text_muted),
                )
                .child(
                    div()
                        .text_size(ui_text_md(cx))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(p.text_secondary)
                        .child(i18n!(cx, "service.no_match")),
                )
                .child(
                    div()
                        .text_size(ui_text_sm(cx))
                        .text_color(p.text_muted)
                        .child(i18n!(cx, "service.no_match_hint")),
                )
                .into_any_element()
        } else {
            let card_min_w = 240.0_f32;
            let gap_w = 12.0_f32;
            let cols = ((cur_w + gap_w) / (card_min_w + gap_w)).floor().max(1.0) as usize;

            v_flex()
                .w_full()
                .gap(SPACE_MD)
                .children(filtered.chunks(cols).map(|chunk| {
                    h_flex()
                        .w_full()
                        .gap(SPACE_MD)
                        .children(chunk.iter().map(|s| {
                            let status = status_of(&s.id);
                            let is_probing = self.service_probing_ids.contains(&s.id);
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(self.render_service_card(s, status, is_probing, t, cx))
                        }))
                        .when(chunk.len() < cols, |row| {
                            let missing = cols - chunk.len();
                            row.children((0..missing).map(|_| div().flex_1().min_w_0()))
                        })
                }))
                .into_any_element()
        };

        v_flex()
            .w_full()
            .gap(SPACE_MD)
            .child(stats_row)
            .child(toolbar)
            .child(content)
            .into_any_element()
    }
}

// Format uptime seconds as "Xd Yh Zm" (days/hours/minutes).
fn format_uptime(secs: u64) -> String {
    let d = secs / 86400;
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    format!("{}天{}时{}分", d, h, m)
}

// Full local date-time "YYYY-MM-DD HH:MM:SS" (UTC fallback).
fn format_datetime() -> String {
    fn fmt(dt: OffsetDateTime) -> String {
        let month: u8 = match dt.month() {
            time::Month::January => 1,
            time::Month::February => 2,
            time::Month::March => 3,
            time::Month::April => 4,
            time::Month::May => 5,
            time::Month::June => 6,
            time::Month::July => 7,
            time::Month::August => 8,
            time::Month::September => 9,
            time::Month::October => 10,
            time::Month::November => 11,
            time::Month::December => 12,
        };
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            dt.year(),
            month,
            dt.day(),
            dt.hour(),
            dt.minute(),
            dt.second()
        )
    }
    match OffsetDateTime::now_local() {
        Ok(now) => fmt(now),
        Err(_) => fmt(OffsetDateTime::now_utc()),
    }
}

// A status-bar item: small icon + monospace value (no text label), matching
// the prototype's `monitor-status-item`.
fn monitor_status_item(
    t: &ThemeColors,
    cx: &App,
    icon: AppIcon,
    value: String,
) -> impl IntoElement {
    h_flex()
        .items_center()
        .gap(SPACE_XS)
        .child(icon.size(ICON_STD).text_color(rgb(t.text_muted)))
        .child(
            div()
                .text_size(ui_text_md(cx))
                .text_color(rgb(t.text_secondary))
                .font_family("monospace")
                .child(value),
        )
}

// Icon + text row (no label), used inside cards (kernel/arch, cores/processes,
// available/swap, IP). Mirrors the prototype's `monitor-icon-text`.
fn monitor_icon_text(t: &ThemeColors, cx: &App, icon: AppIcon, value: String) -> impl IntoElement {
    h_flex()
        .items_center()
        .gap(SPACE_XS)
        .child(icon.size(ICON_MICRO).text_color(rgb(t.text_muted)))
        .child(
            div()
                .text_size(ui_text_ms(cx))
                .text_color(rgb(t.text_secondary))
                .child(value),
        )
}

// A colored usage badge: text tinted by ratio (normal/warning/critical),
// on a subtle neutral surface.
fn monitor_badge(t: &ThemeColors, cx: &App, pct: i32) -> impl IntoElement {
    let fg = if pct >= 80 {
        rgb(t.error)
    } else if pct >= 60 {
        rgb(t.warning)
    } else {
        rgb(t.success)
    };
    div().flex().child(
        div()
            .px(SPACE_SM)
            .py(px(1.0))
            .rounded(RADIUS_MD)
            .bg(surface_bg(t.bg_selection, cx))
            .text_color(fg)
            .text_size(ui_text_ms(cx))
            .font_weight(FontWeight::MEDIUM)
            .font_family("monospace")
            .child(format!("{}%", pct)),
    )
}

// A download/upload row with a tinted background to distinguish it from the
// card body. Mirrors the prototype's `monitor-speed-item`.
fn monitor_speed_item(
    t: &ThemeColors,
    cx: &App,
    icon: AppIcon,
    label: String,
    value: String,
) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .px(SPACE_MD)
        .py(SPACE_SM)
        .rounded(RADIUS_STD)
        .bg(surface_bg(t.bg_selection, cx))
        .child(
            h_flex()
                .gap(SPACE_SM)
                .items_center()
                .child(icon.size(ICON_STD).text_color(rgb(t.text_muted)))
                .child(
                    div()
                        .text_size(ui_text_ms(cx))
                        .text_color(rgb(t.text_muted))
                        .child(label),
                ),
        )
        .child(
            div()
                .text_size(ui_text_md(cx))
                .font_family("monospace")
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(t.text_primary))
                .child(value),
        )
}

impl StatusBar {
    // Disk usage table: header row tinted, every cell carries only a bottom border
    // (no outer left/right/top frame). Matches the redesign mockup.
    fn render_disk_table(&self, t: &ThemeColors, cx: &mut Context<Self>, disks: &[DiskInfo]) -> AnyElement {
        let p = SemanticPalette::from_context(cx);
        let labels: [String; 5] = [
            i18n!(cx, "status.monitor.disk_mount"),
            i18n!(cx, "status.monitor.disk_fs"),
            i18n!(cx, "status.monitor.disk_total"),
            i18n!(cx, "status.monitor.disk_avail"),
            i18n!(cx, "status.monitor.disk_usage"),
        ];

        // Column widths sourced from in-run state (resizable via drag).
        let col_w = |idx: usize| -> f32 {
            self.disk_col_widths
                .get(idx)
                .copied()
                .unwrap_or(DISK_COL_MIN)
        };

        // Header: first 4 columns have right-edge drag handles; the last (usage)
        // flexes to fill the rest of the table width.
        let mut header_cells: Vec<AnyElement> = (0..4)
            .map(|i| {
                let text = labels[i].clone();
                let width = col_w(i);
                let resize_handle = div()
                    .id(("disk-col-resizer", i))
                    .w(px(6.0))
                    .h_full()
                    .cursor_col_resize()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, ev: &MouseDownEvent, _w, cx| {
                            this.disk_col_drag = Some(DiskColDrag {
                                col: i,
                                start_x: ev.position.x,
                                start_w: this
                                    .disk_col_widths
                                    .get(i)
                                    .copied()
                                    .unwrap_or(DISK_COL_MIN),
                            });
                            cx.stop_propagation();
                        }),
                    );

                div()
                    .w(px(width))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_between()
                    .overflow_hidden()
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .px(SPACE_MD)
                            .py(px(5.0))
                            .text_size(ui_text_md(cx))
                            .text_color(rgb(t.text_muted))
                            .child(text),
                    )
                    .child(resize_handle)
                    .into_any_element()
            })
            .collect();

        // 5th column: usage (no resize handle, flex-1).
        header_cells.push(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .px(SPACE_MD)
                .py(px(5.0))
                .text_size(ui_text_md(cx))
                .text_color(rgb(t.text_muted))
                .child(labels[4].clone())
                .into_any_element(),
        );
        let header = div()
            .flex()
            .w_full()
            .bg(surface_bg(t.bg_header, cx))
            .border_1()
            .border_color(p.border_subtle)
            .rounded(RADIUS_STD)
            .overflow_hidden()
            .children(header_cells);

        let rows: Vec<AnyElement> = disks
            .iter()
            .map(|d| {
                let used_pct = if d.total_gb > 0.0 {
                    (((d.total_gb - d.avail_gb) / d.total_gb) * 100.0) as i32
                } else {
                    0
                };
                let values: [String; 4] = [
                    d.mount.clone(),
                    d.fs.clone(),
                    fmt_gb(d.total_gb),
                    fmt_gb(d.avail_gb),
                ];
                let mut cells: Vec<AnyElement> = values
                    .into_iter()
                    .enumerate()
                    .map(|(i, text)| {
                        let is_mount = i == 0;
                        let mut c = div()
                            .w(px(col_w(i)))
                            .flex_shrink_0()
                            .px(SPACE_MD)
                            .py(px(5.0))
                            .text_size(ui_text_md(cx))
                            .text_color(rgb(t.text_secondary))
                            .overflow_hidden();
                        if is_mount {
                            let full = d.mount.clone();
                            c = c.cursor_pointer().on_mouse_down(
                                MouseButton::Left,
                                move |ev: &MouseDownEvent, _window, cx| {
                                    if ev.click_count == 2 {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            full.clone(),
                                        ));
                                        ToastManager::success(
                                            i18n!(cx, "status.monitor.path_copied"),
                                            cx,
                                        );
                                    }
                                },
                            );
                        }
                        c.child(
                            div()
                                .w_full()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .child(text),
                        )
                        .into_any_element()
                    })
                    .collect();
                cells.push(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .px(SPACE_MD)
                        .py(px(5.0))
                        .child(monitor_badge(t, cx, used_pct))
                        .into_any_element(),
                );
                div()
                    .flex()
                    .w_full()
                    .items_center()
                    .border_b_1()
                    .border_color(p.border_subtle)
                    .hover(|s| s.bg(surface_bg(t.bg_hover, cx)))
                    .children(cells)
                    .into_any_element()
            })
            .collect();

        div()
            .flex()
            .flex_col()
            .w_full()
            .overflow_hidden()
            .child(header)
            .children(rows)
            .into_any_element()
    }
}
