mod bottom_dock;
mod dock_views;
mod handlers;
mod pane_switcher;
mod render;
mod terminal_actions;

use crate::settings::settings;
use crate::terminal::backend::{LocalBackend, TerminalBackend};
use crate::terminal::pty_manager::PtyManager;
use crate::views::chrome::title_bar::TitleBar;
use crate::views::layout::split_pane::{ActiveDrag, new_active_drag};
use crate::views::overlays::overlay_manager::OverlayManager;
use crate::views::panels::project_column::ProjectColumn;
use crate::views::panels::status_bar::StatusBar;
use crate::views::panels::toast::ToastOverlay;
use velowork_workspace::dock_controller::DockController;
use crate::workspace::focus::FocusManager;
use crate::workspace::request_broker::RequestBroker;
use crate::workspace::state::{WindowBounds as PersistedWindowBounds, WindowId, Workspace};
use gpui::*;
use parking_lot::Mutex;
use std::cell::RefCell;
use crate::app::detached_overlays::{DetachedHost, DetachedHostCloseEvent, DetachedOverlayOptions, open_detached_overlay};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use velowork_i18n::i18n;
use velowork_ui::icon::AppIcon;
use velowork_ui::overlay_registry::OverlayRegistry;
use velowork_workspace::stores::{GlobalFocusStore, GlobalWindowStore};

/// Shared terminals registry for PTY event routing (re-exported from velowork-terminal)
pub use velowork_terminal::TerminalsRegistry;

/// Registry mapping `terminal_id` to every `TerminalContent` weak handle that
/// renders that terminal. With multiple windows, the same terminal can render
/// in N project-column instances simultaneously (one per window whose visible
/// set includes the host project), so the PTY notify path must fan out to
/// every live entry. Dead weaks are pruned lazily on iteration.
pub type ContentPaneRegistry =
    Arc<Mutex<HashMap<String, Vec<WeakEntity<super::layout::terminal_pane::TerminalContent>>>>>;

/// Global content pane registry instance.
static CONTENT_PANE_REGISTRY: std::sync::OnceLock<ContentPaneRegistry> = std::sync::OnceLock::new();

/// Get or init the global content pane registry.
pub fn content_pane_registry() -> &'static ContentPaneRegistry {
    CONTENT_PANE_REGISTRY.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

/// Notify every weak entity in `weaks` via `cx.notify()`; drop dead weaks in
/// place. Returns `true` if at least one weak was alive (so callers can tell
/// whether any UI update was actually triggered). Generic over the target
/// type so the same helper services the multi-window terminal fan-out and is
/// testable without standing up a `TerminalContent`.
pub fn notify_pane_weaks<T: 'static>(weaks: &mut Vec<WeakEntity<T>>, cx: &mut App) -> bool {
    let mut any_alive = false;
    weaks.retain(|w| match w.update(cx, |_, cx| cx.notify()) {
        Ok(_) => {
            any_alive = true;
            true
        }
        Err(_) => false,
    });
    any_alive
}

/// Per-window view of the application: one instance per OS window.
///
/// Owns the per-window UI state (sidebar, overlays, toasts, scroll handles,
/// drag state, project columns) and addresses window-scoped state on the
/// shared `Workspace` via its own `window_id`. The single OS window opened
/// today hosts a `WindowView` for `WindowId::Main`; slice 05 spawns extras
/// that mint distinct `WindowId::Extra(uuid)`s.
/// Events a `WindowView` raises to the `Velowork` coordinator, which alone owns
/// every window's view + OS handle and can therefore act across windows.
#[derive(Clone)]
pub enum WindowViewEvent {
    /// Jump into an open project's first terminal: activate the window where it
    /// is open (`origin` preferred) and focus its first visible terminal,
    /// leaving the layout untouched.
    JumpToProject {
        origin: WindowId,
        project_id: String,
    },
}

pub struct WindowView {
    /// Identifies which window-scoped slot on the shared `Workspace` this
    /// view addresses (folder filter, hidden set, widths, collapse, focus
    /// zoom). Always `WindowId::Main` in single-window runtime; slice 05
    /// spawns extras that mint distinct `WindowId::Extra(uuid)`s and thread
    /// them in here so each `WindowView` sees only its own per-window state.
    window_id: WindowId,
    /// Per-window focus state: terminal focus stack, project zoom,
    /// fullscreen, modal context. Slice 03 of the multi-window plan moves
    /// this off the shared `Workspace` entity onto each `WindowView` so
    /// every window can zoom and modal-stack independently. Wrapped in
    /// `Entity<FocusManager>` so child views (sidebar, project column,
    /// terminal pane, layout container) can hold a handle to the same
    /// instance and update it through `Entity::update` without needing
    /// to route through `WindowView` first. Workspace action methods that
    /// touched focus state (`set_focused_terminal`, `set_focused_project`,
    /// etc.) now take `focus_manager: &mut FocusManager` as a parameter
    /// so the focus mutation stays scoped to the window driving the action.
    focus_manager: Entity<FocusManager>,
    workspace: Entity<Workspace>,
    request_broker: Entity<RequestBroker>,
    backend: Arc<dyn TerminalBackend>,
    pty_manager: Arc<PtyManager>,
    terminals: TerminalsRegistry,
    sidebar: Entity<crate::views::panels::session_panel::SessionPanel>,
    left_dock: Entity<velowork_ui::dock::DockPanel>,
    /// Left dock state controller
    left_dock_ctrl: DockController,
    /// 右侧 Dock（AI 助手 / 快捷命令 / 隧道）。
    /// 按需创建、至多一个存活：切换离开即置 `None` 销毁 entity 释放内存。
    right_dock: Option<Entity<velowork_ui::dock::DockPanel>>,
    /// 当前激活的右侧工具栏面板 id（IDEA Tool Window 单一激活语义）。
    /// 为 `None` 表示无面板激活。高亮判定由此取代 DockPanel 内部 active_tab_id。
    right_toolbar_active: Option<String>,
    /// 上次打开的右侧工具栏面板 id。隐藏工具栏后仍保留，再次展开时
    /// 若没有当前激活面板则默认重建该面板（记忆上次使用的面板）。
    right_toolbar_last_panel: Option<String>,
    /// Right dock state controller
    right_dock_ctrl: DockController,
    /// Right toolbar registry for dynamic panel registration & visibility management
    right_toolbar_registry: velowork_ui::dock::RightToolbarRegistry,
    /// Controls whether the vertical right toolbar is visible
    right_toolbar_open: bool,

    /// Window-level bottom dock hosting the SFTP and Commands tabs.
    bottom_dock: Entity<velowork_ui::dock::DockPanel>,
    /// Bottom dock state controller
    bottom_dock_ctrl: DockController,
    /// SFTP bottom panel entity (dynamically bound to the focused terminal's
    /// live SSH session). Held so the SFTP tab can be re-added on demand.
    sftp_panel: Entity<crate::views::layout::terminal_pane::sftp_panel::BottomPanel>,
    /// Commands bottom panel entity (terminal-independent broadcaster).
    commands_panel: Entity<crate::views::layout::terminal_pane::commands_panel::CommandsPanel>,
    /// Whether the SFTP tab is currently present in the bottom dock. The SFTP
    /// tab is only shown when the focused terminal is an SSH session with
    /// `enable_sftp: true`.
    sftp_tab_added: bool,
    /// Debounce task for `sync_sftp_tab`.
    sftp_sync_task: Option<gpui::Task<()>>,
    left_dock_anim_task: Option<gpui::Task<()>>,
    right_dock_anim_task: Option<gpui::Task<()>>,
    bottom_dock_anim_task: Option<gpui::Task<()>>,
    left_dock_anim_seq: u64,
    right_dock_anim_seq: u64,
    bottom_dock_anim_seq: u64,
    /// Stored project column entities (created once, not during render)
    project_columns: HashMap<String, Entity<ProjectColumn>>,
    /// Title bar entity
    title_bar: Entity<TitleBar>,
    /// Status bar entity
    status_bar: Entity<StatusBar>,
    /// Centralized overlay manager
    pub(crate) overlay_manager: Entity<OverlayManager>,
    /// Centralized registry of open overlay surfaces (menus, popovers,
    /// tooltips, ...) for window-level click-outside dismissal.
    overlay_registry: Entity<OverlayRegistry>,
    /// Toast notification overlay
    toast_overlay: Entity<ToastOverlay>,
    /// Shared drag state for resize operations
    active_drag: ActiveDrag,
    /// Focus handle for capturing global keybindings
    focus_handle: FocusHandle,
    /// Scroll handle for horizontal scrolling of project columns
    projects_scroll_handle: ScrollHandle,
    /// Persistent container bounds for projects grid (used to compute pixel widths)
    projects_grid_bounds: Rc<RefCell<Bounds<Pixels>>>,
    /// Horizontal scrollbar drag state
    hscroll_dragging: bool,
    hscroll_bounds: Rc<RefCell<Option<Bounds<Pixels>>>>,
    /// Whether the pane switcher overlay is active
    pane_switch_active: bool,
    /// Pane switcher overlay entity (separate entity for proper focus handling)
    pane_switcher_entity: Option<Entity<pane_switcher::PaneSwitcher>>,
    /// Initial titlebar style when window was opened
    pub(crate) initial_titlebar_style: velowork_workspace::settings::TitlebarStyle,
    /// Last focused project ID (for scroll-to-focused detection)
    last_scroll_project: Option<String>,
    /// Whether a project was zoomed/focused in the last observation (for detecting unfocus)
    was_project_focused: bool,
    /// Project ID to center-scroll to after the next layout pass
    pending_center_scroll: Option<String>,
    /// Selected terminal text to inject into the AI assistant panel as a quote
    /// (activated on the next render pass, which has a Window).
    pending_ai_interpret: Option<String>,
    /// Request to open right dock AI assistant panel, scroll to bottom, and focus input.
    pub(crate) pending_ai_open: bool,
    /// Last-known on-disk paths per local project, used to detect renames
    /// so we can refresh cached git providers / service paths.
    last_project_paths: HashMap<String, String>,
    /// Last observed wholesale workspace data replacement epoch.
    last_data_replacement_epoch: u64,
    /// Index into the dock focus cycle (0=Left, 1=Center, 2=Right) for F6 panel cycling.
    cycle_dock_index: usize,
    /// Last window-corner radius pushed to the `WindowCornerRadius` global, so
    /// `modal_backdrop` can clip its dimming mask to the rounded window corners.
    /// Tracked to avoid re-setting the global every frame.
    last_modal_corner_radius: f32,
    /// Flag indicating that a modal was closed and physical focus needs cascade restoration.
    pub(crate) needs_focus_restore: bool,
    /// Modal state in the previous render pass (for modal close detection).
    pub(crate) last_had_modal: bool,
    /// Flag indicating whether the initial cold-start focus has been dispatched.
    pub(crate) initial_focus_done: bool,
    /// Flag allowing force quit when user confirmed quitting with active sessions.
    pub(crate) allow_force_quit: bool,
    /// Whether the window/app is currently in a locked state (screen lock).
    pub(crate) is_locked: bool,
}

impl WindowView {
    pub fn new(
        window_id: WindowId,
        workspace: Entity<Workspace>,
        pty_manager: Arc<PtyManager>,
        terminals: TerminalsRegistry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // Per-window UI request broker. Each window (slice 05 onward) owns its
        // own queue so overlay/sidebar requests stay scoped to the window that
        // produced them; closes slice 03 acceptance criterion that per-window
        // UI entities are constructed inside `WindowView::new` rather than
        // passed in from the `Velowork` singleton.
        let request_broker = cx.new(|_| RequestBroker::new());

        // Per-window focus state: terminal focus stack, project zoom,
        // fullscreen, modal context. Wrapped in Entity<FocusManager> so
        // child views (sidebar, project column, terminal pane, layout
        // container) can hold handles and update through Entity::update.
        let focus_manager = cx.new(|_| FocusManager::new());

        // Restore persisted focused_project_id from the previous session.
        // Only restore if the project still exists in the workspace.
        let default_pid = workspace.read(cx).projects().first().map(|p| p.id.clone());
        let mut restored_pid = None;
        if let Some(window_state) = workspace.read(cx).data().window(window_id) {
            if let Some(ref persisted_pid) = window_state.focused_project_id {
                let project_exists = workspace
                    .read(cx)
                    .projects()
                    .iter()
                    .any(|p| &p.id == persisted_pid);
                if project_exists {
                    restored_pid = Some(persisted_pid.clone());
                }
            }
        }

        let target_pid = restored_pid.or(default_pid);
        if let Some(pid) = target_pid {
            let initial_terminal_path = workspace
                .read(cx)
                .project(&pid)
                .and_then(|p| p.layout.as_ref())
                .map(|l| l.find_visible_terminal_path());

            let workspace_clone = workspace.clone();
            focus_manager.update(cx, |fm, cx| {
                fm.set_active_project_id(Some(pid.clone()));
                fm.set_focused_project_id(Some(pid.clone()));
                if let Some(path) = initial_terminal_path {
                    workspace_clone.update(cx, |ws, cx| {
                        ws.set_focused_terminal(fm, pid, path, cx);
                    });
                }
            });
        }

        // Wire this window's per-window focus manager into the app-level
        // `FocusStore` registry, and register the window itself in
        // `WindowStore`. Both are opt-in: if the stores aren't set up
        // (e.g. some test harnesses) we skip registration rather than panic.
        // The clone before `update` releases the temporary global borrow so
        // `cx` can be taken mutably inside the store callback.
        if let Some(focus_store) = cx.try_global::<GlobalFocusStore>().map(|g| g.0.clone()) {
            focus_store.update(cx, |s, cx| s.register(window_id, focus_manager.clone(), cx));
        }
        if let Some(window_store) = cx.try_global::<GlobalWindowStore>().map(|g| g.0.clone()) {
            let b = window.window_bounds().get_bounds();
            let bounds = PersistedWindowBounds {
                origin_x: f32::from(b.origin.x),
                origin_y: f32::from(b.origin.y),
                width: f32::from(b.size.width),
                height: f32::from(b.size.height),
            };
            window_store.update(cx, |s, cx| s.register(window_id, bounds, cx));
        }

        // Left/right/bottom dock open/closed state is per-window (persisted on WindowState).
        // Seed DockController from the calling window's persisted value;
        // fall back to the global setting for the very first launch where
        // no per-window value exists yet.
        use velowork_ui::dock::DockPosition;
        let app_settings = settings(cx);
        let mut left_dock_ctrl = DockController::new(
            DockPosition::Left,
            app_settings.sidebar.is_open,
            app_settings.sidebar.auto_hide,
            app_settings.sidebar.width,
        );
        if let Some(window_state) = workspace.read(cx).data().window(window_id) {
            let left_open = window_state.is_dock_open(DockPosition::Left, app_settings.sidebar.is_open);
            if left_dock_ctrl.is_open() != left_open {
                left_dock_ctrl.toggle();
                left_dock_ctrl.set_animation(if left_open { 1.0 } else { 0.0 });
            }
        }
        let mut right_dock_ctrl = DockController::new(
            DockPosition::Right,
            app_settings.right_sidebar.is_open,
            app_settings.right_sidebar.auto_hide,
            app_settings.right_sidebar.width,
        );
        if let Some(window_state) = workspace.read(cx).data().window(window_id) {
            let right_open = window_state.is_dock_open(DockPosition::Right, app_settings.right_sidebar.is_open);
            if right_dock_ctrl.is_open() != right_open {
                right_dock_ctrl.toggle();
                right_dock_ctrl.set_animation(if right_open { 1.0 } else { 0.0 });
            }
            let right_size = window_state.dock_size(DockPosition::Right, app_settings.right_sidebar.width);
            right_dock_ctrl.set_size(right_size);
        }
        // 右侧 dock 改为按需创建（IDEA Tool Window 模式）：启动时不默认展开，
        // 由工具栏按钮触发创建专属面板，避免无实体时渲染出空白 dock 区域。
        right_dock_ctrl.set_open(false);
        right_dock_ctrl.set_animation(0.0);

        // Create sidebar entity once to preserve state
        let sidebar = cx.new(|cx| {
            crate::views::panels::session_panel::SessionPanel::new(
                window_id,
                workspace.clone(),
                focus_manager.clone(),
                request_broker.clone(),
                cx,
            )
        });

        // Create overlay manager before the right panel so the panel can hold a
        // handle to it (for opening quick-command dialogs / context menus).
        // Centralized registry of open overlay surfaces (dock menus, popovers,
        // tooltips, ...). Owned here so both `OverlayManager` and the dock
        // panels can reference the same instance; the window routes every
        // left `MouseDown` to it for click-outside dismissal.
        let overlay_registry = cx.new(|_cx| OverlayRegistry::new());
        OverlayRegistry::set_global(overlay_registry.clone(), cx);

        // Hand the registry to the sidebar panel so its footer menus
        // (project / settings) register for click-outside dismissal.
        sidebar.update(cx, |sp, _cx| {
            sp.set_overlay_registry(overlay_registry.clone())
        });

        let window_handle = window.window_handle();
        let overlay_manager = cx.new(|_cx| {
            OverlayManager::new(
                window_id,
                Some(window_handle),
                workspace.clone(),
                focus_manager.clone(),
                request_broker.clone(),
                overlay_registry.clone(),
            )
        });

        sidebar.update(cx, |sp, cx| {
            sp.set_overlay_manager(overlay_manager.clone(), cx);
        });

        // NOTE: the window-level `MouseDown` dispatcher that drives
        // click-outside dismissal is registered in `render` (inside the
        // existing canvas paint closure), because `Window::on_mouse_event`
        // asserts it runs during paint and only lasts one frame.

        // Right dock (AI 助手 / 快捷命令 / 隧道) 改为按需创建：初始为 None，
        // 由 `create_right_panel` 在点击工具栏按钮时按需构造专属单 tab 面板，
        // 切换离开即置 None 销毁 entity。三个面板不再预创建。

        let right_dock_ctrl_width_init = right_dock_ctrl.width();
        let _ = right_dock_ctrl_width_init; // 宽度在 create_right_panel 时按需读取
        let right_dock: Option<Entity<velowork_ui::dock::DockPanel>> = None;

        let left_dock_size_init = left_dock_ctrl.size();
        let left_dock = cx.new(|cx| {
            let mut dp = velowork_ui::dock::DockPanel::new(
                "left_dock",
                left_dock_size_init,
                cx,
            );
            dp.set_header_mode(velowork_ui::dock::DockHeaderMode::TitleOnly);
            dp.set_resize_edge(velowork_ui::dock::ResizeEdge::Right, cx);
            dp.set_overlay_registry(overlay_registry.clone());
            dp.add_tab(velowork_ui::dock::AnyPanel::new(sidebar.clone()), cx);
            // `sidebar` is moved into a sibling closure later in this block, so
            // capture a clone here for the panel factory below (used to reopen
            // the explorer panel from the tab context menu after Ctrl+W).
            let sidebar_for_factory = sidebar.clone();
            dp.set_panel_providers(vec![
                velowork_ui::dock::PanelProvider {
                    id: "explorer".to_string(),
                    title: i18n!(cx, "workspace.explorer"),
                    icon: AppIcon::Folder,
                },
            ]);
            // The explorer panel IS the sidebar. It is added as the initial tab
            // above, but also exposed as a toggleable provider so it can be
            // re-opened from the tab context menu after being closed (Ctrl+W).
            // Without this factory `add_panel_by_id` bails out (no provider)
            // and the panel can never reappear.
            dp.set_on_request_panel(move |id, _window, _cx| match id {
                "explorer" => velowork_ui::dock::AnyPanel::new(sidebar_for_factory.clone()),
                _ => panic!("unknown left dock panel id: {}", id),
            });
            dp
        });

        // Right dock 改为按需创建（Option），不再在初始化时参与 peer_docks 关系；
        // 其专属 DockPanel 在 create_right_panel 中创建，单一实例且单 tab。

        // Create title bar entity (sync initial sidebar state)
        let sidebar_initially_open = left_dock_ctrl.is_open();
        let right_dock_initially_open = right_dock_ctrl.is_open();
        let weak_self = cx.entity().downgrade();
        let title_bar = cx.new(|cx| {
            let mut tb = TitleBar::new("Velowork");
            tb.set_sidebar_open(sidebar_initially_open, cx);
            tb.set_right_sidebar_open(right_dock_initially_open, cx);
            tb.set_close_handler(move |window, cx| {
                if let Some(wv) = weak_self.upgrade() {
                    wv.update(cx, |v, cx| {
                        v.request_close_window(window, cx);
                    });
                } else {
                    window.remove_window();
                }
            });
            tb
        });

        // Wrap PtyManager in LocalBackend for the TerminalBackend trait
        // (created before the status bar so it can feed the SSH probe source).
        let backend: Arc<dyn TerminalBackend> = Arc::new(LocalBackend::new(pty_manager.clone()));

        // Create status bar entity (sync initial sidebar state)
        let workspace_for_status = workspace.clone();
        let focus_manager_for_status = focus_manager.clone();
        let overlay_manager_for_status = overlay_manager.clone();
        let terminals_for_status = terminals.clone();
        let status_bar = cx.new(|cx| {
            let mut sb = StatusBar::new(
                workspace_for_status,
                focus_manager_for_status,
                backend.clone(),
                overlay_manager_for_status,
                terminals_for_status,
                cx,
            );
            sb.set_sidebar_open(sidebar_initially_open, cx);
            sb
        });

        // Create toast overlay
        let toast_overlay = cx.new(ToastOverlay::new);

        // Subscribe to overlay manager events
        cx.subscribe(&overlay_manager, Self::handle_overlay_manager_event)
            .detach();

        // Subscribe to toast action clicks (soft-close undo / close-now).
        cx.subscribe(&toast_overlay, Self::handle_toast_action)
            .detach();

        // Observe RequestBroker to process overlay + terminal-send requests
        // outside of render().
        cx.observe(&request_broker, |this, _broker, cx| {
            let broker = this.request_broker.read(cx);
            let has_overlay = broker.has_overlay_requests();
            let has_send = broker.has_send_to_terminal();
            if has_overlay {
                this.process_pending_requests(cx);
            }
            if has_send {
                this.process_pending_send_to_terminal(cx);
            }
        })
        .detach();

        // Observe the shared project-hover state so this window re-renders its
        // project panels when the hovered project changes — including hovers
        // driven from another window's Switch Project overlay (multi-window
        // panel highlight).
        if let Some(hover) = cx
            .try_global::<crate::views::overlays::project_hover::GlobalProjectHover>()
            .map(|g| g.0.clone())
        {
            cx.observe(&hover, |_this, _hover, cx| cx.notify()).detach();
        }

        // Re-render the whole window whenever the global theme changes so that
        // switching the color schema (Dark/Light/System) or a dark/light palette
        // applies immediately — without restarting the app. The settings
        // observer in `init_theme` mutates the theme entity and notifies it; this
        // subscription forwards that to a WindowView re-render, which cascades to
        // every child view that reads `theme(cx)`.
        cx.observe(&crate::theme::theme_entity(cx), |_this, _theme, cx| {
            cx.notify();
        })
        .detach();

        // Re-render the window whenever the sidebar updates, ensuring sidebar dialog
        // animations (new/edit session modal) re-render smoothly in real time.
        cx.observe(&sidebar, |_this, _sidebar, cx| {
            cx.notify();
        })
        .detach();

        // Re-render the window whenever the overlay manager updates.
        cx.observe(&overlay_manager, |_this, _om, cx| {
            cx.notify();
        })
        .detach();

        // Create focus handle for global keybindings
        let focus_handle = cx.focus_handle();

        // Window-level bottom dock (SFTP + Commands). Hoisted here from
        // ProjectColumn so the whole center region shares a single instance;
        // its content follows the focused terminal regardless of project.
        let (bottom_dock, sftp_panel, commands_panel) = bottom_dock::build_bottom_dock(
            workspace.clone(),
            focus_manager.clone(),
            terminals.clone(),
            backend.clone(),
            overlay_registry.clone(),
            cx,
        );

        let mut bottom_dock_ctrl = DockController::new(DockPosition::Bottom, false, false, bottom_dock.read(cx).size);
        {
            let dock = bottom_dock.read(cx);
            bottom_dock_ctrl.set_width(dock.size);
            let is_bottom_open =
                dock.collapse_state == velowork_ui::dock::PanelCollapseState::Normal;
            if !is_bottom_open {
                if bottom_dock_ctrl.is_open() {
                    bottom_dock_ctrl.toggle();
                }
                bottom_dock_ctrl.set_animation(0.0);
            } else {
                if !bottom_dock_ctrl.is_open() {
                    bottom_dock_ctrl.toggle();
                }
                bottom_dock_ctrl.set_animation(1.0);
            }
        }

        let self_weak = cx.entity().downgrade();
        overlay_manager.update(cx, |om, _cx| {
            let sw = self_weak.clone();
            om.on_open_commands_with = Some(std::sync::Arc::new(move |cmd, cx| {
                if let Some(wv) = sw.upgrade() {
                    wv.update(cx, |this, cx| {
                        this.open_commands_with(&cmd, cx);
                    });
                }
            }));
        });

        let last_data_replacement_epoch = workspace.read(cx).data_replacement_epoch();

        cx.subscribe(
            &sidebar,
            |this, _, event: &crate::views::panels::session_panel::SessionPanelEvent, cx| {
                match event {
                    crate::views::panels::session_panel::SessionPanelEvent::SpawnTerminals {
                        project_id,
                    } => {
                        this.spawn_terminals_for_project(project_id.clone(), cx);
                    }
                    crate::views::panels::session_panel::SessionPanelEvent::DialogChanged => {}
                }
            },
        )
        .detach();

        // Observe settings entity to dynamically update toolbar panel visibility and close hidden panel
        cx.observe(&crate::settings::settings_entity(cx), |this, _, cx| {
            let new_toolbar_open = crate::settings::settings_entity(cx).read(cx).settings.right_toolbar_open;
            if this.right_toolbar_open != new_toolbar_open {
                this.right_toolbar_open = new_toolbar_open;
            }
            if let Some(active_id) = this.right_toolbar_active.clone() {
                if !this.right_toolbar_registry.is_panel_visible(&active_id, cx) {
                    this.right_toolbar_active = None;
                    if this.right_dock_ctrl.is_open() {
                        this.toggle_right_dock(cx);
                    }
                }
            }
            cx.notify();
        })
        .detach();

        let right_toolbar_registry = crate::views::panels::create_default_right_toolbar_registry();

        let mut view = Self {
            window_id,
            focus_manager,
            workspace,
            request_broker,
            backend,
            pty_manager,
            terminals,
            sidebar,
            left_dock,
            left_dock_ctrl,
            right_dock,
            right_dock_ctrl,
            right_toolbar_active: None,
            right_toolbar_last_panel: None,
            right_toolbar_registry,
            right_toolbar_open: app_settings.right_toolbar_open,
            bottom_dock,
            bottom_dock_ctrl,
            sftp_panel,
            commands_panel,
            sftp_tab_added: false,
            sftp_sync_task: None,
            left_dock_anim_task: None,
            right_dock_anim_task: None,
            bottom_dock_anim_task: None,
            left_dock_anim_seq: 0,
            right_dock_anim_seq: 0,
            bottom_dock_anim_seq: 0,
            project_columns: HashMap::new(),
            title_bar,
            status_bar,
            overlay_manager,
            overlay_registry,
            toast_overlay,

            active_drag: new_active_drag(),
            focus_handle,
            projects_scroll_handle: ScrollHandle::new(),
            projects_grid_bounds: Rc::new(RefCell::new(Bounds {
                origin: Point::default(),
                size: Size {
                    width: px(800.0),
                    height: px(600.0),
                },
            })),
            hscroll_dragging: false,
            hscroll_bounds: Rc::new(RefCell::new(None)),
            initial_titlebar_style: crate::settings::settings_entity(cx)
                .read(cx)
                .settings
                .titlebar_style,
            pane_switch_active: false,
            pane_switcher_entity: None,
            last_scroll_project: None,
            was_project_focused: false,
            pending_center_scroll: None,
            pending_ai_interpret: None,
            pending_ai_open: false,
            last_project_paths: HashMap::new(),
            last_data_replacement_epoch,
            cycle_dock_index: 1,
            last_modal_corner_radius: -1.0,
            needs_focus_restore: false,
            last_had_modal: false,
            initial_focus_done: false,
            allow_force_quit: false,
            is_locked: false,
        };

        // Slice 07 cri 7: persist OS bounds back into this window's
        // `WindowState.os_bounds` whenever GPUI reports a bounds change
        // (move, resize, snap, monitor switch). The setter delegates to
        // `data.set_os_bounds` which silently no-ops on an unknown extra id
        // (close-race contract), so a debounced bounds-observer firing on
        // a window that's just been closed is safe. The auto-save observer
        // in `Velowork::new` debounces persistence at 500ms; this observer
        // just bumps `data_version` per bounds change and lets the save
        // path coalesce. Conversion mirrors the inverse path in
        // `src/app/extras.rs::open_extra_window` (gpui `Bounds<Pixels>` ->
        // `PersistedWindowBounds` via four `f32::from(...)` calls).
        cx.observe_window_bounds(window, |this, window, cx| {
            let bounds = window.window_bounds().get_bounds();
            let persisted = PersistedWindowBounds {
                origin_x: f32::from(bounds.origin.x),
                origin_y: f32::from(bounds.origin.y),
                width: f32::from(bounds.size.width),
                height: f32::from(bounds.size.height),
            };
            let window_id = this.window_id;
            this.workspace.update(cx, |ws, cx| {
                ws.set_os_bounds(window_id, Some(persisted.clone()), cx);
            });
            // Mirror the live OS bounds into WindowStore so the app-level
            // window registry stays in sync with real geometry. This is the
            // single, traceable write point for window bounds — consumers
            // read WindowStore instead of re-deriving from the Workspace.
            if let Some(window_store) = cx.try_global::<GlobalWindowStore>().map(|g| g.0.clone()) {
                window_store.update(cx, |s, cx| s.set_bounds(window_id, persisted, cx));
            }
        })
        .detach();

        // React to OS window activation/deactivation so the app-level focus
        // registry (`FocusStore`) and window registry (`WindowStore`) stay in
        // sync with real OS focus. This is the single, traceable write point
        // for window focus state — consumers read the stores instead of
        // re-deriving focus from the `Workspace` or blind observers.
        cx.observe_window_activation(window, |this, window, cx| {
            let window_id = this.window_id;
            let focused = window.is_window_active();
            if let Some(window_store) = cx.try_global::<GlobalWindowStore>().map(|g| g.0.clone()) {
                window_store.update(cx, |s, cx| s.set_focused(window_id, focused, cx));
            }
            if focused {
                if let Some(focus_store) = cx.try_global::<GlobalFocusStore>().map(|g| g.0.clone())
                {
                    focus_store.update(cx, |s, cx| s.set_window_focused(window_id, cx));
                }
            }
        })
        .detach();

        // Hook the OS window close so the app-level window registry
        // (`WindowStore`) is cleared exactly when the window goes away — the
        // single traceable write point for the `Closed` event. Return `true`
        // to let the close proceed, or `false` to intercept and prompt confirmation.
        let wid = window_id;
        let view_handle = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            if let Some(view) = view_handle.upgrade() {
                if wid == WindowId::Main {
                    let (allow, session_count, terminal_count) = view.update(cx, |v, cx| {
                        (v.allow_force_quit, v.active_session_count(cx), v.open_terminal_count())
                    });

                    if !allow && (session_count > 0 || terminal_count > 0) {
                        window.activate_window();
                        window.refresh();
                        view.update(cx, |v, cx| {
                            v.request_quit(window, cx);
                        });
                        return false;
                    }

                    Self::flush_all_state(cx);
                    if let Some(window_store) = cx.try_global::<GlobalWindowStore>().map(|g| g.0.clone()) {
                        window_store.update(cx, |s, cx| s.close(wid, cx));
                    }
                    cx.quit();
                    return true;
                }
            }

            if let Some(window_store) = cx.try_global::<GlobalWindowStore>().map(|g| g.0.clone()) {
                window_store.update(cx, |s, cx| s.close(wid, cx));
            }
            true
        });

        // Observe focus_manager to scroll focused project into view.
        // (Workspace observers no longer fire on focus changes since
        // focus moved off the Workspace entity in slice 03.)
        cx.observe(&view.focus_manager, |this, fm, cx| {
            let fm = fm.read(cx);
            let is_project_focused = fm.focused_project_id().is_some();
            let active_project_id = fm.active_project_id().cloned();
            let focused_terminal_project =
                fm.focused_terminal_state().map(|f| f.project_id.clone());

            // When project zoom is cleared, defer centering until after next layout pass
            if this.was_project_focused && !is_project_focused {
                this.last_scroll_project = focused_terminal_project.clone();
                this.pending_center_scroll = focused_terminal_project;
            }
            // When the active terminal changes project, ensure it's visible
            else if focused_terminal_project != this.last_scroll_project
                && focused_terminal_project.is_some()
            {
                this.last_scroll_project = focused_terminal_project.clone();
                this.scroll_to_focused_project(focused_terminal_project.as_deref(), false, cx);
            }

            this.was_project_focused = is_project_focused;

            // Persist active_project_id into WindowState so it survives restarts.
            let wid = this.window_id;
            this.workspace.update(cx, |ws, cx| {
                ws.persist_focused_project_id(wid, active_project_id, cx);
            });

            // Re-sync the SFTP tab whenever the focused terminal changes so the
            // tab reflects the focused connection's `enable_sftp` setting.
            this.schedule_sync_sftp_tab(cx);
            cx.notify();
        })
        .detach();

        // Observe workspace data changes so project path renames refresh
        // cached git providers / service paths.
        cx.observe(&view.workspace, |this, _workspace, cx| {
            let data_replacement_epoch = this.workspace.read(cx).data_replacement_epoch();
            if this.last_data_replacement_epoch != data_replacement_epoch {
                this.last_data_replacement_epoch = data_replacement_epoch;
                this.project_columns.clear();
                this.last_project_paths.clear();
                let wid = this.window_id;
                let preferred = this.workspace.read(cx).data().window(wid).and_then(|w| w.focused_project_id.clone());
                let projects = this.workspace.read(cx).projects().to_vec();
                this.focus_manager.update(cx, |fm, cx| {
                    fm.clear_all();
                    fm.realign_with_projects(&projects, preferred.as_deref());
                    cx.notify();
                });
                this.sync_project_columns(cx);
            }
            this.refresh_for_project_path_changes(cx);
        })
        .detach();

        // Initialize project columns
        view.sync_project_columns(cx);

        // Seed path snapshot so the observer only fires on real changes.
        view.last_project_paths = view.snapshot_local_project_paths(cx);

        view.setup_dock_subscriptions(view.left_dock.clone(), cx);

        // 窗口挂载就绪首帧：执行初始物理焦点对齐，确保启动后即刻处于有焦状态
        let initial_view = cx.entity().downgrade();
        window.on_next_frame(move |window, cx| {
            if window.focused(cx).is_none() {
                if let Some(view) = initial_view.upgrade() {
                    view.update(cx, |this, cx| {
                        this.focus_active_terminal(window, cx);
                    });
                }
            }
        });

        view
    }

    /// Force-close every interactive floating surface currently open in this
    /// window (modals, context menus, dropdown panels, popovers, ...). Used by
    /// the auto-lock flow so nothing is left visible behind the lock screen.
    pub fn close_all_overlays(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.overlay_manager.update(cx, |om, cx| {
            om.close_all_overlays(window, cx);
        });
    }

    /// Update the window's screen lock state.
    pub(crate) fn set_locked(&mut self, locked: bool, cx: &mut Context<Self>) {
        if self.is_locked != locked {
            self.is_locked = locked;
            cx.notify();
        }
    }

    /// Get the terminals registry (for sharing with detached windows).
    // Forward-looking API for slice 05 (multi-window): detached windows will
    // share this registry. Unused until then.
    #[allow(dead_code)]
    pub fn terminals(&self) -> &TerminalsRegistry {
        &self.terminals
    }

    /// Identifies which window-scoped slot on the shared `Workspace` this
    /// view addresses. Always `WindowId::Main` today (single-window runtime);
    /// slice 05 spawns extras that mint distinct `WindowId::Extra(uuid)`s.
    /// Field is read directly within the impl via `self.window_id`; this
    /// public getter exists for external callers (e.g. the slice 05 spawn
    /// flow on `Velowork`) that need to address window-scoped state on
    /// `Workspace` in the same window this view inhabits.
    #[allow(dead_code)]
    pub fn window_id(&self) -> WindowId {
        self.window_id
    }

    /// Per-window focus state, owned by this WindowView. Returned as an
    /// `Entity<FocusManager>` handle so callers (children, sibling views)
    /// can `update`/`read` it without going through `WindowView`. Workspace
    /// action methods that mutate focus (`set_focused_terminal`,
    /// `set_focused_project`, etc.) take `&mut FocusManager` as a parameter,
    /// supplied via `focus_manager.update(cx, |fm, cx| ws.method(fm, ...))`.
    pub fn focus_manager(&self) -> Entity<FocusManager> {
        self.focus_manager.clone()
    }

    /// Find the active or registered AiAssistantPanel in the right dock if available.
    pub fn find_ai_assistant_panel(
        &self,
        cx: &App,
    ) -> Option<Entity<crate::views::panels::ai_assistant_panel::AiAssistantPanel>> {
        let dock = self.right_dock.as_ref()?;
        for tab in &dock.read(cx).tabs {
            if let Ok(ai) = tab
                .view
                .clone()
                .downcast::<crate::views::panels::ai_assistant_panel::AiAssistantPanel>()
            {
                return Some(ai);
            }
        }
        None
    }

    /// Snapshot current on-disk paths for local projects (keyed by project_id).
    fn snapshot_local_project_paths(&self, cx: &Context<Self>) -> HashMap<String, String> {
        self.workspace
            .read(cx)
            .projects()
            .iter()
            .filter(|p| !p.is_remote)
            .map(|p| (p.id.clone(), p.path.clone()))
            .collect()
    }

    /// Detect local project directory renames and refresh caches that hold a
    /// snapshotted path (git provider inside GitHeader).
    fn refresh_for_project_path_changes(&mut self, cx: &mut Context<Self>) {
        let current = self.snapshot_local_project_paths(cx);

        let changed: Vec<(String, String)> = current
            .iter()
            .filter(|(id, path)| self.last_project_paths.get(id.as_str()) != Some(*path))
            .map(|(id, path)| (id.clone(), path.clone()))
            .collect();

        if changed.is_empty() {
            // Still drop entries for projects that no longer exist
            if current.len() != self.last_project_paths.len() {
                self.last_project_paths = current;
            }
            return;
        }

        self.last_project_paths = current;
    }

    /// Ensure project columns exist for all visible projects
    fn sync_project_columns(&mut self, cx: &mut Context<Self>) {
        let visible_projects: Vec<(String, bool, Option<String>)> = {
            let ws = self.workspace.read(cx);
            let fm = self.focus_manager.read(cx);
            ws.visible_projects(
                self.window_id,
                fm.focused_project_id(),
                fm.is_focus_individual(),
            )
            .iter()
            .map(|p| (p.id.clone(), p.is_remote, p.connection_id.clone()))
            .collect()
        };

        // Clean up columns for projects that no longer exist
        let visible_ids: std::collections::HashSet<&str> = visible_projects
            .iter()
            .map(|(id, _, _)| id.as_str())
            .collect();
        self.project_columns
            .retain(|id, _| visible_ids.contains(id.as_str()));

        // Create columns for new projects
        for (project_id, _is_remote, _connection_id) in &visible_projects {
            if !self.project_columns.contains_key(project_id) {
                let entity = Some(self.create_local_column(project_id, cx));
                if let Some(entity) = entity {
                    self.project_columns.insert(project_id.clone(), entity);
                }
            }
        }
    }

    /// Create a ProjectColumn for a local project.
    fn create_local_column(
        &self,
        project_id: &str,
        cx: &mut Context<Self>,
    ) -> Entity<ProjectColumn> {
        let workspace_clone = self.workspace.clone();
        let focus_manager_clone = self.focus_manager.clone();
        let request_broker_clone = self.request_broker.clone();
        let terminals_clone = self.terminals.clone();
        let active_drag_clone = self.active_drag.clone();
        let id = project_id.to_string();
        let backend_clone = self.backend.clone();
        let workspace_for_dispatch = self.workspace.clone();
        let focus_manager_for_dispatch = self.focus_manager.clone();
        let backend_for_dispatch = self.backend.clone();
        let terminals_for_dispatch = self.terminals.clone();

        let window_id = self.window_id;
        let entity = cx.new(move |cx| {
            let mut col = ProjectColumn::new(
                window_id,
                workspace_clone,
                focus_manager_clone,
                request_broker_clone,
                id,
                backend_clone,
                terminals_clone,
                active_drag_clone,
                self.overlay_registry.clone(),
                cx,
            );
            col.set_action_dispatcher(Some(crate::action_dispatch::ActionDispatcher::Local {
                workspace: workspace_for_dispatch,
                focus_manager: focus_manager_for_dispatch,
                backend: backend_for_dispatch,
                terminals: terminals_for_dispatch,
                window_id,
                overlay_manager: Some(self.overlay_manager.clone()),
            }));
            col
        });

        entity
    }

    /// 构建右侧 dock 面板的 provider 列表（动态由 RightToolbarRegistry 提供）。
    /// 供 `create_right_panel`、`spawn_detached_panel_window` 和
    /// `spawn_detached_whole_panel_window` 共用，保证所有右侧 dock（含分离窗口）
    /// 的右键菜单都能显示面板切换选项。
    fn right_dock_panel_providers(
        registry: &velowork_ui::dock::RightToolbarRegistry,
        cx: &App,
    ) -> Vec<velowork_ui::dock::PanelProvider> {
        registry.panel_providers(cx)
    }

    /// 构建右侧 dock 面板的工厂闭包（`on_request_panel`），按 id 经由注册中心动态创建对应面板。
    /// 供 `create_right_panel`、`spawn_detached_panel_window` 和
    /// `spawn_detached_whole_panel_window` 共用。
    fn make_right_dock_request_panel_factory(
        registry: velowork_ui::dock::RightToolbarRegistry,
        workspace: Entity<Workspace>,
        focus_manager: Entity<FocusManager>,
        terminals: TerminalsRegistry,
        overlay_manager: Entity<OverlayManager>,
        overlay_registry: Entity<OverlayRegistry>,
    ) -> impl Fn(&str, &mut Window, &mut App) -> velowork_ui::dock::AnyPanel + Send + Sync + 'static {
        move |id, window, cx| {
            let ctx = velowork_ui::dock::PanelCreationContext::new(
                crate::views::panels::AppPanelCreationContext {
                    workspace: workspace.clone(),
                    focus_manager: focus_manager.clone(),
                    terminals: terminals.clone(),
                    overlay_manager: overlay_manager.clone(),
                    overlay_registry: overlay_registry.clone(),
                },
            );
            registry
                .create_panel(id, &ctx, window, cx)
                .unwrap_or_else(|| panic!("unknown panel id: {}", id))
        }
    }

    /// 按需创建右侧专属 DockPanel（IDEA Tool Window 单一激活模式）。
    ///
    /// 每个 `panel_id` 对应一个独立的 `DockPanel`（内部仅含该按钮的专属 tab）。
    /// 调用方在切换离开旧面板时先将其置 `None` 丢弃强引用，GPUI 在引用归零后
    /// 自动销毁 entity 释放内存。新面板在此订阅 detach/hide/whole 与宽度同步事件。
    fn create_right_panel(
        &mut self,
        panel_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<velowork_ui::dock::DockPanel> {
        let width = self.right_dock_ctrl.width();
        let factory = Self::make_right_dock_request_panel_factory(
            self.right_toolbar_registry.clone(),
            self.workspace.clone(),
            self.focus_manager.clone(),
            self.terminals.clone(),
            self.overlay_manager.clone(),
            self.overlay_registry.clone(),
        );
        let providers = Self::right_dock_panel_providers(&self.right_toolbar_registry, cx);
        let dp = cx.new(|cx| {
            let mut dp = velowork_ui::dock::DockPanel::new("right_dock", width, cx);
            dp.set_header_mode(velowork_ui::dock::DockHeaderMode::TitleOnly);
            dp.set_resize_edge(velowork_ui::dock::ResizeEdge::Left, cx);

            // Register the panels that can be toggled from this dock's "more"
            // menu. 即使当前仅显示单一 tab，保留 providers 以便通过 "more" 选择器切换。
            dp.set_panel_providers(providers);

            // 工厂闭包：按 id 返回对应面板 view 的 AnyPanel。
            dp.set_on_request_panel(factory);

            dp.activate_or_add_panel(panel_id, window, cx);
            dp
        });


        // Hand the overlay registry to the dock so its "more" / tab menus
        // register themselves for centralized click-outside dismissal.
        dp.update(cx, |d, _cx| {
            d.set_overlay_registry(self.overlay_registry.clone())
        });

        // Wire the status-bar's "Open Service Manager" entry to this dock.
        self.status_bar
            .update(cx, |sb, _cx| sb.set_right_dock(Some(dp.clone())));

        // 订阅该专属 dock 的 detach / hide / whole 事件（在 entity 创建时各自订阅，
        // 因 panel 按需创建、切换即销毁，无法在初始化时统一订阅固定实体）。
        cx.subscribe(
            &dp,
            |this, dock, event: &velowork_ui::dock::DockPanelDetachEvent, cx| {
                let panel_id = event.id.clone();
                this.spawn_detached_panel_window(
                    velowork_ui::dock::DockPosition::Right,
                    dock.clone(),
                    &panel_id,
                    cx,
                );
                if dock.read(cx).tabs.is_empty() {
                    // Panel fully left the main window: drop the toolbar
                    // highlight so the detached panel doesn't leave a stuck
                    // active button pointing at a panel that is no longer here;
                    // its re-attach will re-set the highlight itself.
                    if this.right_toolbar_active.as_deref() == Some(panel_id.as_str()) {
                        this.right_toolbar_active = None;
                    }
                    if this.right_dock_ctrl.is_open() {
                        this.toggle_right_dock(cx);
                    }
                }
            },
        )
        .detach();

        cx.subscribe(
            &dp,
            |this, _, _event: &velowork_ui::dock::DockPanelHideEvent, cx| {
                // 隐藏面板即视为该专属面板收起，必须清除工具栏高亮，否则按钮
                // 会残留选中背景，且下次点击走 toggle-off 分支只切换 dock 而不
                // 重建面板，最终显示空白面板。
                this.right_toolbar_active = None;
                if this.right_dock_ctrl.is_open() {
                    this.toggle_right_dock(cx);
                }
            },
        )
        .detach();

        cx.subscribe(
            &dp,
            |this, dock, event: &velowork_ui::dock::DockPanelDetachWholeEvent, cx| {
                let tabs = event.tabs.clone();
                let active = event.active_tab_index;
                this.spawn_detached_whole_panel_window(
                    velowork_ui::dock::DockPosition::Right,
                    dock.clone(),
                    tabs,
                    active,
                    cx,
                );
                // 整体分离后清除工具栏高亮，与单 tab 分离处理器一致。
                // 附加回主窗口时由 reattach 逻辑重新设置。
                this.right_toolbar_active = None;
                if this.right_dock_ctrl.is_open() {
                    this.toggle_right_dock(cx);
                }
            },
        )
        .detach();

        // 宽度同步：拖拽调整右侧 dock 宽度时回写 controller 与设置。
        // 同时检测专属面板标签是否被用户在面板内关闭：右侧 dock 为单一 tab
        // 的专属面板，标签被关闭后 `right_toolbar_active` 必须立即清除，否则
        // 工具栏按钮会残留选中背景，且再次点击只能显示空白面板。
        let right_panel_id = panel_id.to_string();
        cx.subscribe(
            &dp,
            move |this, sidebar, _event: &velowork_ui::dock::DockPanelEvent, cx| {
                let width = sidebar.read(cx).size;
                this.right_dock_ctrl.set_width(width);
                crate::settings::settings_entity(cx)
                    .update(cx, |s, cx| s.set_right_sidebar_width(width, cx));
                if this.right_toolbar_active.as_deref() == Some(right_panel_id.as_str())
                    && !sidebar
                        .read(cx)
                        .tabs
                        .iter()
                        .any(|t| t.metadata(cx).id.0 == right_panel_id)
                {
                    // 标签已关闭：清除高亮并收起 dock，使其回到"未展开"状态。
                    this.right_toolbar_active = None;
                    if this.right_dock_ctrl.is_open() {
                        this.toggle_right_dock(cx);
                    }
                }
                cx.notify();
            },
        )
        .detach();

        dp
    }

    fn setup_dock_subscriptions(
        &mut self,
        left_dock: Entity<velowork_ui::dock::DockPanel>,
        cx: &mut Context<Self>,
    ) {
        // 右侧 dock 的订阅（detach/hide/whole + 宽度同步）已迁移至
        // `create_right_panel`，因为每个右侧 dock 实体按需创建、切换即销毁，
        // 需在创建时各自订阅，而非在此对固定实体订阅。
        cx.subscribe(
            &left_dock,
            |this, dock, _event: &velowork_ui::dock::DockPanelEvent, cx| {
                let size = dock.read(cx).size;
                this.left_dock_ctrl.set_size(size);
                crate::settings::settings_entity(cx)
                    .update(cx, |s, cx| s.set_sidebar_width(size, cx));
                cx.notify();
            },
        )
        .detach();

        cx.subscribe(
            &left_dock,
            |this, dock, event: &velowork_ui::dock::DockPanelDetachEvent, cx| {
                let panel_id = event.id.clone();
                this.spawn_detached_panel_window(
                    velowork_ui::dock::DockPosition::Left,
                    dock.clone(),
                    &panel_id,
                    cx,
                );
                if dock.read(cx).tabs.is_empty() {
                    if this.left_dock_ctrl.is_open() {
                        this.toggle_left_dock(cx);
                    }
                }
            },
        )
        .detach();

        cx.subscribe(
            &left_dock,
            |this, dock, _event: &velowork_ui::dock::DockPanelHideEvent, cx| {
                // 关闭最后一个标签后 dock 收起：补回默认的"资源器"标签，使下次
                // 通过 Ctrl+B 或状态栏按钮重新展开时面板正常显示，而非空白。
                if dock.read(cx).tabs.is_empty() {
                    dock.update(cx, |dp, cx| {
                        dp.add_tab(velowork_ui::dock::AnyPanel::new(this.sidebar.clone()), cx);
                    });
                }
                if this.left_dock_ctrl.is_open() {
                    this.toggle_left_dock(cx);
                }
            },
        )
        .detach();

        cx.subscribe(
            &left_dock,
            |this, dock, event: &velowork_ui::dock::DockPanelDetachWholeEvent, cx| {
                let tabs = event.tabs.clone();
                let active = event.active_tab_index;
                this.spawn_detached_whole_panel_window(
                    velowork_ui::dock::DockPosition::Left,
                    dock.clone(),
                    tabs,
                    active,
                    cx,
                );
                if this.left_dock_ctrl.is_open() {
                    this.toggle_left_dock(cx);
                }
            },
        )
        .detach();
    }

    /// Spawn a standalone OS window containing a single detached dock panel.
    /// Sets up the on_attach callback to attach it back to the main window when clicked.
    fn spawn_detached_panel_window(
        &self,
        pos: velowork_ui::dock::DockPosition,
        main_dock: Entity<velowork_ui::dock::DockPanel>,
        panel_id: &str,
        cx: &mut Context<Self>,
    ) {
        let workspace = self.workspace.clone();
        let focus_manager = self.focus_manager.clone();
        let request_broker = self.request_broker.clone();
        let terminals = self.terminals.clone();
        let overlay_manager = self.overlay_manager.clone();
        let overlay_registry = self.overlay_registry.clone();
        let window_id = self.window_id;
        let panel_id_owned = panel_id.to_string();

        let main_dock_clone = main_dock.clone();
        let main_window_view = cx.entity().clone();

        let title_text = match panel_id {
            "explorer" => "Explorer - Detached",
            "quick_commands" => "Quick Commands - Detached",
            "ai_assistant" => "AI Assistant - Detached",
            "tunnels" => "Tunnels - Detached",
            "services" => "Service Monitor - Detached",
            _ => "Detached Panel",
        };
        let factory_workspace = workspace.clone();
        let factory_focus = focus_manager.clone();
        let factory_terminals = terminals.clone();
        let factory_overlay = overlay_manager.clone();
        let factory_overlay_reg = overlay_registry.clone();

        let builder_workspace = workspace.clone();
        let builder_focus = focus_manager.clone();
        let builder_request_broker = request_broker.clone();
        let builder_terminals = terminals.clone();
        let builder_overlay = overlay_manager.clone();
        let pid_for_close = panel_id_owned.clone();

        let reattach = Arc::new(move |pid: &str, _window: &mut Window, cx: &mut App| {
            let pid_owned = pid.to_string();
            let workspace = workspace.clone();
            let focus_manager = focus_manager.clone();
            let request_broker = request_broker.clone();
            let terminals = terminals.clone();
            let overlay_manager = overlay_manager.clone();
            let overlay_registry = overlay_registry.clone();

            let _ = main_window_view.update(cx, |view, cx| {
                let panel_entity = main_dock.clone();
                match pos {
                    velowork_ui::dock::DockPosition::Right => {
                        if view.right_dock.is_none() {
                            view.right_dock = Some(panel_entity);
                        }
                    }
                    _ => {}
                }

                let dock_field = match pos {
                    velowork_ui::dock::DockPosition::Left => Some(&view.left_dock),
                    velowork_ui::dock::DockPosition::Right => view.right_dock.as_ref(),
                    _ => None,
                };

                if let Some(dock) = dock_field {
                    let p = match pid_owned.as_str() {
                        "explorer" => {
                            let inner = cx.new(|cx| {
                                let mut sp = crate::views::panels::session_panel::SessionPanel::new(
                                    window_id,
                                    workspace.clone(),
                                    focus_manager.clone(),
                                    request_broker.clone(),
                                    cx,
                                );
                                sp.set_overlay_registry(overlay_registry.clone());
                                sp.set_overlay_manager(overlay_manager.clone(), cx);
                                sp
                            });
                            velowork_ui::dock::AnyPanel::new(inner)
                        }
                        "quick_commands" => {
                            let inner = cx.new(|cx| {
                                let mut qp = crate::views::panels::quick_commands_panel::QuickCommandsPanel::new(
                                    workspace.clone(),
                                    focus_manager.clone(),
                                    terminals.clone(),
                                    overlay_manager.clone(),
                                    cx,
                                );
                                qp.set_overlay_registry(overlay_registry.clone());
                                qp
                            });
                            velowork_ui::dock::AnyPanel::new(inner)
                        }
                        "ai_assistant" => {
                            let inner = cx.new(|cx| {
                                crate::views::panels::ai_assistant_panel::AiAssistantPanel::new(
                                    workspace.clone(),
                                    focus_manager.clone(),
                                    terminals.clone(),
                                    overlay_manager.clone(),
                                    cx,
                                )
                            });
                            velowork_ui::dock::AnyPanel::new(inner)
                        }
                        "tunnels" => {
                            let inner = cx.new(|cx| {
                                let mut tp = crate::views::panels::tunnels_panel::TunnelsPanel::new(
                                    workspace.clone(),
                                    focus_manager.clone(),
                                    overlay_manager.clone(),
                                    cx,
                                );
                                tp.set_overlay_registry(overlay_registry.clone());
                                tp
                            });
                            velowork_ui::dock::AnyPanel::new(inner)
                        }
                        "services" => {
                            let inner = cx.new(|cx| {
                                crate::views::panels::service_monitor_panel::ServiceMonitorPanel::new(
                                    workspace.clone(),
                                    focus_manager.clone(),
                                    terminals.clone(),
                                    overlay_manager.clone(),
                                    cx,
                                )
                            });
                            velowork_ui::dock::AnyPanel::new(inner)
                        }
                        _ => panic!("unknown panel id: {}", pid_owned),
                    };
                    dock.update(cx, |sidebar, cx| {
                        sidebar.add_tab(p, cx);
                    });
                }

                match pos {
                    velowork_ui::dock::DockPosition::Left => {
                        if !view.left_dock_ctrl.is_open() {
                            view.toggle_left_dock(cx);
                        } else {
                            view.left_dock_ctrl.set_open(true);
                            cx.notify();
                        }
                    }
                    velowork_ui::dock::DockPosition::Right => {
                        view.right_toolbar_active = Some(pid_owned.clone());
                        view.right_toolbar_last_panel = Some(pid_owned.clone());
                        if !view.right_dock_ctrl.is_open() {
                            view.toggle_right_dock(cx);
                        } else {
                            view.right_dock_ctrl.set_open(true);
                            cx.notify();
                        }
                    }
                    _ => {}
                }
            });
        });

        let reattach_close = reattach.clone();

        let _ = open_detached_overlay::<DetachedHost<velowork_ui::dock::DockPanel>, DetachedHostCloseEvent>(
            title_text,
            move |_window, overlay_registry, cx| {
                let main_dock_for_peer = main_dock_clone.clone();

                // Build the initial panel instance for the detached dock
                let panel = match panel_id_owned.as_str() {
                    "explorer" => {
                        let p = cx.new(|cx| {
                            let mut sp = crate::views::panels::session_panel::SessionPanel::new(
                                window_id,
                                builder_workspace.clone(),
                                builder_focus.clone(),
                                builder_request_broker.clone(),
                                cx,
                            );
                            sp.set_overlay_registry(overlay_registry.clone());
                            sp
                        });
                        velowork_ui::dock::AnyPanel::new(p)
                    }
                    "quick_commands" => {
                        let p = cx.new(|cx| {
                            let mut qp = crate::views::panels::quick_commands_panel::QuickCommandsPanel::new(
                                builder_workspace.clone(),
                                builder_focus.clone(),
                                builder_terminals.clone(),
                                builder_overlay.clone(),
                                cx,
                            );
                            qp.set_overlay_registry(overlay_registry.clone());
                            qp
                        });
                        velowork_ui::dock::AnyPanel::new(p)
                    }
                    "ai_assistant" => {
                        let p = cx.new(|cx| {
                            crate::views::panels::ai_assistant_panel::AiAssistantPanel::new(
                                builder_workspace.clone(),
                                builder_focus.clone(),
                                builder_terminals.clone(),
                                builder_overlay.clone(),
                                cx,
                            )
                        });
                        velowork_ui::dock::AnyPanel::new(p)
                    }
                    "tunnels" => {
                        let p = cx.new(|cx| {
                            let mut tp = crate::views::panels::tunnels_panel::TunnelsPanel::new(
                                builder_workspace.clone(),
                                builder_focus.clone(),
                                builder_overlay.clone(),
                                cx,
                            );
                            tp.set_overlay_registry(overlay_registry.clone());
                            tp
                        });
                        velowork_ui::dock::AnyPanel::new(p)
                    }
                    "services" => {
                        let p = cx.new(|cx| {
                            crate::views::panels::service_monitor_panel::ServiceMonitorPanel::new(
                                builder_workspace.clone(),
                                builder_focus.clone(),
                                builder_terminals.clone(),
                                builder_overlay.clone(),
                                cx,
                            )
                        });
                        velowork_ui::dock::AnyPanel::new(p)
                    }
                    _ => panic!("unknown panel id: {}", panel_id_owned),
                };

                let detached_dock = cx.new(|cx| {
                    let mut dp = velowork_ui::dock::DockPanel::new("detached_panel_dock", 400.0, cx);
                    dp.add_tab(panel, cx);
                    // Fill the whole detached OS window so it resizes in both
                    // width and height and inner content adapts without gaps.
                    dp.fill_window = true;
                    dp.set_overlay_registry(overlay_registry.clone());
                    // 设置 panel providers 和工厂闭包，使分离窗口的右键菜单
                    // 也能显示面板切换选项。
                    dp.set_panel_providers(WindowView::right_dock_panel_providers(&self.right_toolbar_registry, cx));
                    dp.set_on_request_panel(WindowView::make_right_dock_request_panel_factory(
                        self.right_toolbar_registry.clone(),
                        factory_workspace,
                        factory_focus,
                        factory_terminals,
                        factory_overlay,
                        factory_overlay_reg,
                    ));

                    // Peer link so the single-dock rule is enforced across the
                    // main dock and this detached window: opening the
                    // panel back in the main dock pulls it out of here.
                    dp.set_peer_docks(vec![main_dock_for_peer.downgrade()]);

                    let reattach = reattach.clone();
                    dp.set_on_attach(move |pid, window, cx| reattach(pid, window, cx));
                    dp
                });

                // Make the main dock aware of this detached window so its
                // "more" menu picker enforces the single-dock rule against it.
                let detached_dock_weak = detached_dock.downgrade();
                let _ = main_dock_for_peer.clone().update(cx, |dp, _cx| {
                    dp.peer_docks.push(detached_dock_weak);
                });

                cx.new(|_cx| DetachedHost {
                    inner: detached_dock,
                })
            },
            DetachedOverlayOptions {
                size: gpui::size(gpui::px(400.0), gpui::px(600.0)),
                min_size: gpui::Size {
                    width: gpui::px(250.0),
                    height: gpui::px(300.0),
                },
                on_close: Some(Arc::new(move |window, cx| reattach_close(&pid_for_close, window, cx))),
                hide_titlebar: true,
            },
            cx,
        );
    }

    /// Spawn a standalone OS window that contains the ENTIRE panel — every tab
    /// of the source dock — rather than a single tab. The provided `AnyPanel`s
    /// are clones of the source dock's tabs, so they share the underlying panel
    /// entities and each panel's internal state, tab order and active selection
    /// are carried over intact. The window renders a `fill_window` dock with all
    /// tabs; closing it (or using its "attach" action) re-merges every tab back
    /// into the main dock.
    fn spawn_detached_whole_panel_window(
        &self,
        pos: velowork_ui::dock::DockPosition,
        main_dock: Entity<velowork_ui::dock::DockPanel>,
        tabs: Vec<velowork_ui::dock::AnyPanel>,
        active_tab_index: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        let main_dock_clone = main_dock.clone();
        let main_window_view = cx.entity().clone();

        let detached_dock_ref: Arc<Mutex<Option<Entity<velowork_ui::dock::DockPanel>>>> =
            Arc::new(Mutex::new(None));
        let detached_dock_ref_clone = detached_dock_ref.clone();

        let main_dock_reattach = main_dock_clone.clone();
        let main_window_view_reattach = main_window_view.clone();

        let reattach_all = Arc::new(move |_pid: &str, _window: &mut Window, cx: &mut App| {
            if let Some(detached_dock) = detached_dock_ref_clone.lock().clone() {
                let tabs = detached_dock.read(cx).tabs.clone();
                if !tabs.is_empty() {
                    let active = detached_dock.read(cx).active_tab_index;
                    // Determine the active panel id from the detached dock's active tab
                    // so we can restore right_toolbar_active on re-attach.
                    let active_panel_id = {
                        let idx = active.unwrap_or(0).min(tabs.len().saturating_sub(1));
                        tabs[idx].metadata(cx).id.0.clone()
                    };
                    let _ = main_dock_reattach.update(cx, move |sidebar, cx| {
                        for tab in tabs {
                            sidebar.add_tab(tab, cx);
                        }
                        if let Some(active) = active {
                            let idx = active.min(sidebar.tabs.len().saturating_sub(1));
                            sidebar.select_tab(idx, cx);
                        }
                    });
                    detached_dock.update(cx, |d, _cx| {
                        d.tabs.clear();
                        d.active_tab_index = None;
                    });
                    // Only re-open the main dock when at least one tab was
                    // actually re-attached. If every tab was closed in the
                    // detached window the user clearly wants that panel gone,
                    // so we must NOT force-open an empty dock (which would show
                    // a blank panel). Mirrors the terminal detach behaviour.
                    let _ = main_window_view_reattach.update(cx, |view, cx| {
                        match pos {
                            velowork_ui::dock::DockPosition::Left => {
                                if !view.left_dock_ctrl.is_open() {
                                    view.toggle_left_dock(cx);
                                }
                            }
                            velowork_ui::dock::DockPosition::Right => {
                                // 恢复 right_dock 字段：分离后 close 动画会将其置
                                // None，必须在这里重新指向旧 DockPanel entity（它
                                // 一直被闭包持有、实体仍活跃），否则渲染时
                                // right_dock.is_none() → 空白面板。
                                if view.right_dock.is_none() {
                                    view.right_dock = Some(main_dock_reattach.clone());
                                }
                                // 恢复工具栏高亮和记忆面板。
                                view.right_toolbar_active = Some(active_panel_id.clone());
                                view.right_toolbar_last_panel = Some(active_panel_id);
                                if !view.right_dock_ctrl.is_open() {
                                    view.toggle_right_dock(cx);
                                } else {
                                    view.right_dock_ctrl.set_open(true);
                                    cx.notify();
                                }
                            }
                            _ => {}
                        }
                    });
                }
            }
        });

        let reattach_on_attach = reattach_all.clone();
        let reattach_on_close = reattach_all.clone();

        // Clone entities for the detached dock's panel factory (context menu
        // panel toggle support).
        let factory_workspace = self.workspace.clone();
        let factory_focus = self.focus_manager.clone();
        let factory_terminals = self.terminals.clone();
        let factory_overlay = self.overlay_manager.clone();
        let factory_overlay_reg = self.overlay_registry.clone();
        let factory_registry = self.right_toolbar_registry.clone();

        let _ = open_detached_overlay::<DetachedHost<velowork_ui::dock::DockPanel>, DetachedHostCloseEvent>(
            "Detached Panel",
            move |_window, overlay_registry, cx| {
                let main_dock_for_peer = main_dock_clone.clone();

                let detached_dock = cx.new(|cx| {
                    let mut dp =
                        velowork_ui::dock::DockPanel::new("detached_panel_dock", 400.0, cx);
                    for tab in tabs {
                        dp.add_tab(tab, cx);
                    }
                    if let Some(active) = active_tab_index {
                        dp.select_tab(active.min(dp.tabs.len().saturating_sub(1)), cx);
                    }
                    dp.fill_window = true;
                    dp.set_overlay_registry(overlay_registry.clone());
                    dp.set_peer_docks(vec![main_dock_for_peer.downgrade()]);
                    // 设置 panel providers 和工厂闭包，使分离窗口的右键菜单
                    // 也能显示面板切换选项。
                    dp.set_panel_providers(WindowView::right_dock_panel_providers(&factory_registry, cx));
                    dp.set_on_request_panel(WindowView::make_right_dock_request_panel_factory(
                        factory_registry.clone(),
                        factory_workspace,
                        factory_focus,
                        factory_terminals,
                        factory_overlay,
                        factory_overlay_reg,
                    ));
                    dp
                });


                *detached_dock_ref.lock() = Some(detached_dock.clone());

                let detached_dock_weak = detached_dock.downgrade();
                let _ = main_dock_for_peer.clone().update(cx, |dp, _cx| {
                    dp.peer_docks.push(detached_dock_weak);
                });

                let reattach_on_attach = reattach_on_attach.clone();
                let _ = detached_dock.update(cx, |dp, _cx| {
                    dp.set_on_attach(move |pid, window, cx| reattach_on_attach(pid, window, cx));
                });

                cx.new(|_cx| DetachedHost {
                    inner: detached_dock,
                })
            },
            DetachedOverlayOptions {
                size: gpui::size(gpui::px(400.0), gpui::px(600.0)),
                min_size: gpui::Size {
                    width: gpui::px(250.0),
                    height: gpui::px(300.0),
                },
                on_close: Some(Arc::new(move |window, cx| reattach_on_close("", window, cx))),
                hide_titlebar: true,
            },
            cx,
        );
    }
}

impl_focusable!(WindowView);

impl EventEmitter<WindowViewEvent> for WindowView {}

#[cfg(test)]
mod tests {
    use super::notify_pane_weaks;
    use gpui::AppContext as _;

    struct Stub;

    #[gpui::test]
    fn fans_out_to_every_alive_weak_and_prunes_dead(cx: &mut gpui::TestAppContext) {
        let (a, b, mut weaks) = cx.update(|cx| {
            let a = cx.new(|_| Stub);
            let b = cx.new(|_| Stub);
            let weaks = vec![a.downgrade(), b.downgrade()];
            (a, b, weaks)
        });

        cx.update(|cx| {
            assert!(notify_pane_weaks(&mut weaks, cx));
            assert_eq!(weaks.len(), 2, "both alive entries kept");
        });

        drop(b);

        cx.update(|cx| {
            assert!(notify_pane_weaks(&mut weaks, cx));
            assert_eq!(weaks.len(), 1, "dead entry pruned, live entry kept");
        });

        drop(a);

        cx.update(|cx| {
            assert!(!notify_pane_weaks(&mut weaks, cx));
            assert!(weaks.is_empty(), "all dead entries pruned");
        });
    }
}
