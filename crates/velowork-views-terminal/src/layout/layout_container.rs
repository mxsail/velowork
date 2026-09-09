//! Recursive layout container that renders terminal/split/tabs nodes

use crate::ActionDispatch;
use velowork_ui::icon::AppIcon;
use velowork_core::api::ActionRequest;
use velowork_i18n::i18n;
use velowork_terminal::backend::TerminalBackend;
use velowork_ui::theme::theme;
use velowork_ui::menu::PopupMenu;
use velowork_ui::overlay_menu::OverlayMenu;
use velowork_ui::overlay_registry::OverlayRegistry;
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::theme::{bg_opacity, with_alpha};
use velowork_ui::tokens::SPACE_MD;
use velowork_ui::click_detector::ClickDetector;
use crate::layout::pane_drag::{PaneDrag, DropZone};
use crate::layout::split_pane::{ActiveDrag, render_split_divider};
use crate::layout::terminal_pane::TerminalPane;
use velowork_terminal::TerminalsRegistry;
use velowork_workspace::focus::FocusManager;
use velowork_workspace::request_broker::RequestBroker;
use velowork_workspace::state::{LayoutNode, SplitDirection, WindowId, Workspace};
use velowork_workspace::stores::GlobalConnectionStore;
use gpui::*;
use gpui::prelude::*;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

// Re-export rename state from velowork-ui
pub use velowork_ui::rename_state::*;

/// Recursive layout container that renders terminal/split/tabs nodes
pub struct LayoutContainer<D: ActionDispatch> {
    pub(super) workspace: Entity<Workspace>,
    pub(super) focus_manager: Entity<FocusManager>,
    pub(super) request_broker: Entity<RequestBroker>,
    pub(super) window_id: WindowId,
    pub(super) project_id: String,
    pub(super) project_path: String,
    pub(super) layout_path: Vec<usize>,
    pub(super) backend: Arc<dyn TerminalBackend>,
    pub(super) terminals: TerminalsRegistry,
    pub(super) terminal_pane: Option<Entity<TerminalPane<D>>>,
    pub(super) child_containers: HashMap<Vec<usize>, Entity<LayoutContainer<D>>>,
    pub(super) container_bounds_ref: Rc<RefCell<Bounds<Pixels>>>,
    /// Global bounds of the tab-list toggle button, captured each paint so the
    /// searchable tab-list dropdown can be anchored directly below it.
    pub(super) tab_list_btn_bounds: Rc<RefCell<Bounds<Pixels>>>,
    pub(super) drop_animation: Option<(usize, f32)>,
    pub(super) active_drag: ActiveDrag,
    pub(super) tab_click_detector: ClickDetector<usize>,
    pub(super) empty_area_click_detector: ClickDetector<()>,
    pub(super) tab_rename_state: Option<RenameState<String>>,
    pub(super) action_dispatcher: Option<D>,
    pub(super) tab_scroll_handle: ScrollHandle,
    pub(super) last_scrolled_to_tab: Option<usize>,
    /// Searchable tab-list dropdown menu (`OverlayMenu`). `None` when closed.
    pub(super) tab_list_menu: Option<Entity<OverlayMenu>>,
    pub(super) tab_list_toggle_guard: Option<std::time::Instant>,
    /// Set after the one-time connection-status subscriptions/poll are
    /// installed in `render` (the constructor has no `cx` to do it with).
    pub(super) connection_init_done: bool,
    /// Active header "more" dropdown menu (e.g. split vertical/horizontal), mirroring
    /// the dock panel's more menu. `None` when closed.
    pub(super) more_menu: Option<Entity<PopupMenu>>,
    pub(super) more_menu_toggle_guard: Option<std::time::Instant>,
    /// Captured bounds of the header "more" button, so the dropdown anchors
    /// directly below it.
    pub(super) more_button_bounds: Rc<RefCell<Bounds<Pixels>>>,
    /// Window-level `OverlayRegistry` used for click-outside dismissal of the
    /// header "more" dropdown (mirrors `DockPanel`). Injected by `ProjectColumn`
    /// (top-level) and propagated to child split containers.
    pub(super) overlay_registry: Option<Entity<OverlayRegistry>>,
    pub(super) background_cache_observed: bool,
    /// Index of the tab currently hovered by the mouse, used to reveal its
    /// close button (hidden by default). `None` when no tab is hovered.
    pub(super) hovered_tab: Option<usize>,
}

/// Build the single shared terminal-area background.
///
/// Combines the theme's `term_background` color with the optional background
/// image on top, applying the global background opacity to the whole group
/// exactly ONCE (via `.opacity(bg_opacity(cx))`); the color and image layers
/// inside are opaque. This avoids double-applying `bg_opacity` (once on the
/// color layer and again on the image layer), which previously made the
/// terminal look ~2x more opaque than the rest of the app — its transparency to
/// the desktop was `(1-opacity)²` instead of `(1-opacity)`. Now the terminal
/// background composites as a single translucent surface, matching every other
/// UI surface.
///
/// This is the canonical terminal backdrop. It is painted exactly ONCE by the
/// owning central region (`ProjectColumn`) as the bottom-most layer, beneath
/// both the terminal `LayoutContainer` and the empty / creating placeholder
/// states, so the backdrop looks identical whether or not a terminal is attached
/// yet and the global transparency is never applied more than once.
pub fn render_terminal_shared_background(
    cx: &App,
    radius: f32,
    bottom_only: bool,
) -> AnyElement {
    let t = theme(cx);
    let theme_palette = SemanticPalette::from_theme(&t);
    let container = div()
        .absolute()
        .inset_0()
        .size_full()
        .overflow_hidden()
        .opacity(bg_opacity(cx));

    let el = match crate::background_cache::terminal_background_element_with_corners(
        cx,
        "terminal-area-bg".into(),
        false,
        radius,
        bottom_only,
    ) {
        // When an image is present, the image element itself renders the authoritative
        // rounded corners and Cover texture. Do NOT stack a solid color quad beneath it,
        // which would cause double antialiasing (sub-pixel fringing) around corner curves.
        Some(img) => container.child(img),
        None => {
            // Tab bar height matches `bar_height` in `render_tab_bar`:
            // `(tab_height - 6.0).max(24.0) + SPACE_SM * 2.0`
            let item_height = px((velowork_ui::tab::tab_height(cx) - 6.0).max(24.0));
            let bar_height = item_height + velowork_ui::tokens::SPACE_SM * 2.0;

            let top_corners = !bottom_only && radius > 0.0;
            let bottom_corners = radius > 0.0;

            container
                .flex()
                .flex_col()
                .child(
                    // Top zone: Tab bar header backdrop (dark surface)
                    div()
                        .w_full()
                        .h(bar_height)
                        .bg(theme_palette.surface_header)
                        .when(top_corners, |d| d.rounded_t(px(radius))),
                )
                .child(
                    // Bottom zone: Base card surface backdrop behind terminal panes.
                    // Each foreground terminal pane renders its own session color scheme
                    // and handles its respective bottom corner radii.
                    div()
                        .flex_1()
                        .w_full()
                        .min_h_0()
                        .bg(theme_palette.surface_card)
                        .when(bottom_corners, |d| {
                            d.rounded_bl(px(radius)).rounded_br(px(radius))
                        }),
                )
        }
    };
    el.into_any_element()
}

impl<D: ActionDispatch + Send + Sync> LayoutContainer<D> {
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
        backend: Arc<dyn TerminalBackend>,
        terminals: TerminalsRegistry,
        active_drag: ActiveDrag,
        action_dispatcher: Option<D>,
    ) -> Self {
        Self {
            workspace,
            focus_manager,
            request_broker,
            window_id,
            project_id,
            project_path,
            layout_path,
            backend,
            terminals,
            terminal_pane: None,
            child_containers: HashMap::new(),
            container_bounds_ref: Rc::new(RefCell::new(Bounds {
                origin: Point::default(),
                size: Size { width: px(800.0), height: px(600.0) },
            })),
            tab_list_btn_bounds: Rc::new(RefCell::new(Bounds {
                origin: Point::default(),
                size: Size { width: px(0.0), height: px(0.0) },
            })),
            drop_animation: None,
            active_drag,
            tab_click_detector: ClickDetector::new(),
            empty_area_click_detector: ClickDetector::new(),
            tab_rename_state: None,
            action_dispatcher,
            tab_scroll_handle: ScrollHandle::new(),
            last_scrolled_to_tab: None,
            tab_list_menu: None,
            tab_list_toggle_guard: None,
            connection_init_done: false,
            more_menu: None,
            more_menu_toggle_guard: None,
            more_button_bounds: Rc::new(RefCell::new(Bounds::default())),
            overlay_registry: None,
            background_cache_observed: false,
            hovered_tab: None,
        }
    }

    /// Inject the window-level `OverlayRegistry` so the header "more" dropdown
    /// registers for centralized click-outside dismissal (same as `DockPanel`).
    pub fn set_overlay_registry(&mut self, reg: Entity<OverlayRegistry>) {
        self.overlay_registry = Some(reg);
    }

    /// Whether a terminal's connection (or underlying process) has been lost.
    ///
    /// * SSH terminals: disconnected when the live SSH session handle is closed
    ///   *or* the connection store no longer tracks the session as connected
    ///   (e.g. after the last tab for that session was closed).
    /// * Local terminals: disconnected when the shell process has exited
    ///   (`shell_pid` is `None`).
    pub(super) fn is_terminal_connection_lost(&self, tid: &str, cx: &App) -> bool {
        if let Some(s) = self.backend.get_ssh_session(tid) {
            if let Some(sid) = self.backend.get_ssh_session_id(tid) {
                let store_connected = cx
                    .global::<GlobalConnectionStore>()
                    .0
                    .read(cx)
                    .is_connected(&sid);
                !store_connected || s.is_closed()
            } else {
                s.is_closed()
            }
        } else if let Some(sid) = self.backend.get_ssh_session_id(tid) {
            let store_connected = cx
                .global::<GlobalConnectionStore>()
                .0
                .read(cx)
                .is_connected(&sid);
            !store_connected
        } else {
            let lock = self.terminals.lock();
            if let Some(t) = lock.get(tid) {
                t.shell_pid().is_none()
            } else {
                !tid.starts_with("welcome:")
            }
        }
    }

    /// Re-render whenever the global connection store changes (connect /
    /// disconnect) so per-tab status dots update the instant a session state
    /// flips — e.g. the red "disconnected" dot appears as soon as the last
    /// terminal for a session is closed.
    fn observe_connection_events(&self, cx: &mut Context<Self>) {
        let store = cx.global::<GlobalConnectionStore>().0.clone();
        cx.subscribe(
            &store,
            |_, _, _: &velowork_workspace::stores::ConnectionEvent, cx| {
                cx.notify();
            },
        )
        .detach();
    }

    /// Lightweight poll that re-renders this pane when any terminal's live
    /// connection state changes — covering cases the `ConnectionEvent` store
    /// doesn't emit (e.g. an unexpected SSH network drop where the russh handle
    /// closes but `mark_disconnected` is never called, or a local shell
    /// exiting). Real-time within ~1.5s without depending on external events.
    fn start_connection_watch(&self, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        cx.spawn(async move |_this, cx| {
            let mut last: Option<String> = None;
            loop {
                smol::Timer::after(std::time::Duration::from_millis(1500)).await;
                let result = weak.update(cx, |this, cx| {
                    let sig = this.connection_signature(cx);
                    if last.as_deref() != Some(sig.as_str()) {
                        last = Some(sig);
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

    /// A compact signature of every terminal's connection-lost state. Two
    /// identical signatures mean no tab's status dot needs to change.
    fn connection_signature(&self, cx: &App) -> String {
        let tids: Vec<String> = {
            let registry = self.terminals.lock();
            registry.keys().cloned().collect()
        };
        let mut parts: Vec<String> = Vec::with_capacity(tids.len());
        for tid in &tids {
            parts.push(format!(
                "{}:{}",
                tid,
                self.is_terminal_connection_lost(tid, cx) as u8
            ));
        }
        parts.sort();
        parts.join(",")
    }

    pub fn set_project_path(&mut self, path: String) {
        self.project_path = path;
    }

    /// Whether this project's layout contains a `Split` node — i.e. the
    /// terminal area is showing more than one pane at once ("分屏"). Single-pane
    /// projects (even with several terminals stacked behind inactive tabs)
    /// return `false`.
    #[allow(dead_code)]
    pub(super) fn is_project_split(&self, cx: &App) -> bool {
        self.workspace
            .read(cx)
            .project(&self.project_id)
            .and_then(|p| p.layout.as_ref())
            .is_some_and(|layout| layout.has_split())
    }

    /// Whether this pane is the currently focused terminal in the unified
    /// `FocusManager` — the "active" region of a split.
    #[allow(dead_code)]
    pub(super) fn is_active_pane(&self, cx: &App) -> bool {
        self.focus_manager
            .read(cx)
            .is_focused(&self.project_id, &self.layout_path)
    }

    /// Dynamically resolve the active `TerminalPane` entity for this container,
    /// traversing child tab/split containers if this node is a tab group or split.
    pub fn active_terminal_pane(&self, cx: &App) -> Option<Entity<TerminalPane<D>>> {
        if let Some(ref pane) = self.terminal_pane {
            return Some(pane.clone());
        }

        let ws = self.workspace.read(cx);
        let layout_node = self.get_layout(ws)?;

        match layout_node {
            LayoutNode::Tabs { children, active_tab } => {
                if children.get(*active_tab).is_some() {
                    let mut child_path = self.layout_path.clone();
                    child_path.push(*active_tab);
                    if let Some(child_container) = self.child_containers.get(&child_path) {
                        return child_container.read(cx).active_terminal_pane(cx);
                    }
                }
            }
            LayoutNode::Split { children, .. } => {
                let fm = self.focus_manager.read(cx);
                if let Some(focused) = fm.focused_terminal_state()
                    && focused.project_id == self.project_id
                    && focused.layout_path.starts_with(&self.layout_path)
                    && focused.layout_path.len() > self.layout_path.len()
                {
                    let child_idx = focused.layout_path[self.layout_path.len()];
                    let mut child_path = self.layout_path.clone();
                    child_path.push(child_idx);
                    if let Some(child_container) = self.child_containers.get(&child_path) {
                        return child_container.read(cx).active_terminal_pane(cx);
                    }
                }
                if !children.is_empty() {
                    let mut child_path = self.layout_path.clone();
                    child_path.push(0);
                    if let Some(child_container) = self.child_containers.get(&child_path) {
                        return child_container.read(cx).active_terminal_pane(cx);
                    }
                }
            }
            _ => {}
        }
        None
    }


    fn ensure_terminal_pane(
        &mut self,
        terminal_id: Option<String>,
        minimized: bool,
        detached: bool,
        cx: &mut Context<Self>,
    ) {
        let needs_new_pane = match &self.terminal_pane {
            None => true,
            Some(pane) => {
                let current_id = pane.read(cx).terminal_id();
                current_id != terminal_id
            }
        };

        if needs_new_pane {
            let workspace = self.workspace.clone();
            let focus_manager = self.focus_manager.clone();
            let request_broker = self.request_broker.clone();
            let window_id = self.window_id;
            let project_id = self.project_id.clone();
            let project_path = self.project_path.clone();
            let layout_path = self.layout_path.clone();
            let backend = self.backend.clone();
            let terminals = self.terminals.clone();
            let remote_ctx = self.action_dispatcher.clone();

            self.terminal_pane = Some(cx.new(move |cx| {
                TerminalPane::new(
                    workspace,
                    focus_manager,
                    request_broker,
                    window_id,
                    project_id,
                    project_path,
                    layout_path,
                    terminal_id,
                    minimized,
                    detached,
                    backend,
                    terminals,
                    remote_ctx,
                    cx,
                )
            }));
        } else if let Some(pane) = &self.terminal_pane {
            pane.update(cx, |pane, cx| {
                pane.set_minimized(minimized, cx);
                pane.set_detached(detached, cx);
            });
        }
    }

    pub(super) fn get_layout<'a>(&self, workspace: &'a Workspace) -> Option<&'a LayoutNode> {
        let project = workspace.project(&self.project_id)?;
        project.layout.as_ref()?.get_at_path(&self.layout_path)
    }


    pub(super) fn find_zoomed_child_index(
        &self,
        children: &[LayoutNode],
        cx: &Context<Self>,
    ) -> Option<usize> {
        let fm = self.focus_manager.read(cx);
        let (fs_project_id, fs_terminal_id) = fm.fullscreen_state()?;
        if fs_project_id != self.project_id {
            return None;
        }

        for (i, child) in children.iter().enumerate() {
            let ids = child.collect_terminal_ids();
            if ids.iter().any(|id| id == fs_terminal_id) {
                return Some(i);
            }
        }
        None
    }

    pub(super) fn deregister_resize_viewers(&mut self, cx: &mut Context<Self>) {
        if let Some(pane) = self.terminal_pane.clone() {
            pane.update(cx, |pane, cx| pane.deregister_resize_viewer(cx));
        }

        let children: Vec<_> = self.child_containers.values().cloned().collect();
        for child in children {
            child.update(cx, |child, cx| child.deregister_resize_viewers(cx));
        }
    }

    pub(super) fn deregister_child_resize_viewers_except(
        &mut self,
        visible_paths: &HashSet<Vec<usize>>,
        cx: &mut Context<Self>,
    ) {
        let hidden_children: Vec<_> = self
            .child_containers
            .iter()
            .filter(|(path, _)| !visible_paths.contains(*path))
            .map(|(_, child)| child.clone())
            .collect();

        for child in hidden_children {
            child.update(cx, |child, cx| child.deregister_resize_viewers(cx));
        }
    }

    fn is_in_tab_group(&self, cx: &Context<Self>) -> bool {
        if self.layout_path.is_empty() {
            return false;
        }
        let parent_path = &self.layout_path[..self.layout_path.len() - 1];
        let ws = self.workspace.read(cx);
        if let Some(project) = ws.project(&self.project_id)
            && let Some(LayoutNode::Tabs { .. }) = project.layout.as_ref().and_then(|l| l.get_at_path(parent_path)) {
                return true;
            }
        false
    }

    pub(super) fn start_tab_rename(
        &mut self,
        terminal_id: String,
        current_name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let placeholder = i18n!(cx, "terminal.tab_name_placeholder");
        self.tab_rename_state = Some(start_rename_with_blur(
            terminal_id,
            &current_name,
            &placeholder,
            |this: &mut LayoutContainer<D>, _window, cx| {
                this.finish_tab_rename(cx);
            },
            window,
            cx,
        ));
        let workspace = self.workspace.clone();
        self.focus_manager.update(cx, |fm, cx| {
            workspace.update(cx, |ws, cx| ws.clear_focused_terminal(fm, cx));
            cx.notify();
        });
        cx.notify();
    }

    pub(super) fn finish_tab_rename(&mut self, cx: &mut Context<Self>) {
        if let Some((terminal_id, new_name)) = finish_rename(&mut self.tab_rename_state, cx)
            && let Some(ref dispatcher) = self.action_dispatcher {
                dispatcher.dispatch(
                    ActionRequest::RenameTerminal {
                        project_id: self.project_id.clone(),
                        terminal_id,
                        name: new_name,
                    },
                    cx,
                );
            }
        let workspace = self.workspace.clone();
        self.focus_manager.update(cx, |fm, cx| {
            workspace.update(cx, |ws, cx| ws.restore_focused_terminal(fm, cx));
            cx.notify();
        });
        cx.notify();
    }

    pub(super) fn cancel_tab_rename(&mut self, cx: &mut Context<Self>) {
        cancel_rename(&mut self.tab_rename_state);
        let workspace = self.workspace.clone();
        self.focus_manager.update(cx, |fm, cx| {
            workspace.update(cx, |ws, cx| ws.restore_focused_terminal(fm, cx));
            cx.notify();
        });
        cx.notify();
    }

    fn render_terminal(
        &mut self,
        terminal_id: Option<String>,
        minimized: bool,
        detached: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        self.ensure_terminal_pane(terminal_id.clone(), minimized, detached, cx);

        let in_tab_group = self.is_in_tab_group(cx);

        let mut container = div()
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .relative();

        // 未处于 tab 组中的独立终端（含拆分面板内的终端），无论是否处于
        // 全屏（zoom）状态都保留其独立顶栏；全屏时顶栏右侧的“全屏展开”
        // 按钮会自动切换为“收起全屏”（见 render_tab_action_buttons）。
        if !in_tab_group {
            container = container.child(self.render_standalone_tab_bar(window, cx));
        }

        if detached || minimized {
            let t = theme(cx);
            let p = SemanticPalette::from_theme(&t);
            let empty_placeholder = div()
                .flex_1()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(SPACE_MD)
                .text_color(p.text_muted)
                .child(
                    AppIcon::Terminal
                        .size(px(28.0))
                        .text_color(p.text_muted),
                )
                .child(
                    div()
                        .text_size(velowork_ui::tokens::ui_text_md(cx))
                        .child(i18n!(cx, "terminal.empty_tabs")),
                );
            return container.child(empty_placeholder);
        }

        container
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .when_some(self.terminal_pane.clone(), |d, pane| {
                        d.child(AnyView::from(pane).cached(
                            StyleRefinement::default().size_full(),
                        ))
                    })
                    .child(self.render_drop_zones(terminal_id, cx, &self.active_drag.clone())),
            )
    }

    fn render_drop_zones(
        &self,
        terminal_id: Option<String>,
        cx: &mut Context<Self>,
        active_drag: &ActiveDrag,
    ) -> impl IntoElement {
        let t = theme(cx);
        let highlight = with_alpha(t.border_active, 0.3);
        let project_id = self.project_id.clone();
        let tid = terminal_id.clone();
        let id_suffix = terminal_id.unwrap_or_else(|| format!("none-{:?}", self.layout_path));
        let dispatcher = self.action_dispatcher.clone();

        let make_zone = |zone: DropZone, id_suffix: &str, active_drag: &ActiveDrag| -> Stateful<Div> {
            let zone_id = format!("drop-zone-{}-{:?}", id_suffix, zone);
            let pid = project_id.clone();
            let this_tid = tid.clone();
            let active_drag_for_hover = active_drag.clone();
            let active_drag_for_drop = active_drag.clone();
            let dispatcher = dispatcher.clone();

            let zone_str = match zone {
                DropZone::Top => "top",
                DropZone::Bottom => "bottom",
                DropZone::Left => "left",
                DropZone::Right => "right",
                DropZone::Center => "center",
            };

            div()
                .id(ElementId::Name(zone_id.into()))
                .drag_over::<PaneDrag>(move |style, _, _, _| {
                    if active_drag_for_hover.borrow().is_some() {
                        return style;
                    }
                    style.bg(highlight)
                })
                .on_drop(cx.listener({
                    let pid = pid.clone();
                    let this_tid = this_tid.clone();
                    move |_this, drag: &PaneDrag, _window, cx| {
                        if active_drag_for_drop.borrow().is_some() {
                            return;
                        }
                        if Some(drag.terminal_id.as_str()) == this_tid.as_deref() {
                            return;
                        }
                        if let Some(ref target_id) = this_tid
                            && let Some(ref dispatcher) = dispatcher {
                                dispatcher.dispatch(ActionRequest::MovePaneTo {
                                    project_id: drag.project_id.clone(),
                                    terminal_id: drag.terminal_id.clone(),
                                    target_project_id: pid.clone(),
                                    target_terminal_id: target_id.clone(),
                                    zone: zone_str.to_string(),
                                }, cx);
                            }
                    }
                }))
        };

        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .flex_row()
            .child(
                make_zone(DropZone::Left, &id_suffix, active_drag)
                    .w(relative(0.25))
                    .h_full(),
            )
            .child(
                div()
                    .w(relative(0.50))
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(
                        make_zone(DropZone::Top, &id_suffix, active_drag)
                            .w_full()
                            .h(relative(0.25)),
                    )
                    .child(
                        make_zone(DropZone::Center, &id_suffix, active_drag)
                            .w_full()
                            .h(relative(0.50)),
                    )
                    .child(
                        make_zone(DropZone::Bottom, &id_suffix, active_drag)
                            .w_full()
                            .h(relative(0.25)),
                    ),
            )
            .child(
                make_zone(DropZone::Right, &id_suffix, active_drag)
                    .w(relative(0.25))
                    .h_full(),
            )
    }

    fn render_split(
        &mut self,
        direction: SplitDirection,
        sizes: &[f32],
        children: &[LayoutNode],
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let num_children = children.len();
        let project_id = self.project_id.clone();
        let layout_path = self.layout_path.clone();

        if let Some(zoomed_idx) = self.find_zoomed_child_index(children, cx) {
            let mut child_path = self.layout_path.clone();
            child_path.push(zoomed_idx);

            let visible_paths = HashSet::from([child_path.clone()]);
            self.deregister_child_resize_viewers_except(&visible_paths, cx);

            let container = self
                .child_containers
                .entry(child_path.clone())
                .or_insert_with(|| {
                    cx.new(|_cx| {
                        LayoutContainer::new(
                            self.workspace.clone(),
                            self.focus_manager.clone(),
                            self.request_broker.clone(),
                            self.window_id,
                            self.project_id.clone(),
                            self.project_path.clone(),
                            child_path.clone(),
                            self.backend.clone(),
                            self.terminals.clone(),
                            self.active_drag.clone(),
                            self.action_dispatcher.clone(),
                        )
                    })
                })
                .clone();

            if let Some(reg) = self.overlay_registry.clone() {
                container.update(cx, |c, _cx| c.set_overlay_registry(reg));
            }

            return div()
                .id(ElementId::Name(format!("split-container-{}-{:?}", project_id, layout_path).into()))
                .size_full()
                .min_h_0()
                .min_w_0()
                .child(AnyView::from(container).cached(
                    StyleRefinement::default().size_full()
                ));
        }

        let is_horizontal = direction == SplitDirection::Horizontal;

        let valid_paths: std::collections::HashSet<Vec<usize>> = (0..num_children)
            .map(|i| {
                let mut path = self.layout_path.clone();
                path.push(i);
                path
            })
            .collect();

        let mut visible_children_info: Vec<(usize, f32)> = Vec::new();
        for (i, child) in children.iter().enumerate() {
            if !child.is_all_hidden() {
                let size = sizes.get(i).copied().unwrap_or(100.0 / num_children as f32);
                visible_children_info.push((i, size));
            }
        }
        let visible_paths: HashSet<Vec<usize>> = visible_children_info
            .iter()
            .map(|(i, _)| {
                let mut path = self.layout_path.clone();
                path.push(*i);
                path
            })
            .collect();
        self.deregister_child_resize_viewers_except(&visible_paths, cx);
        self.child_containers.retain(|path, _| valid_paths.contains(path));

        let container_bounds_ref = self.container_bounds_ref.clone();

        let total_visible_size: f32 = visible_children_info.iter().map(|(_, s)| s).sum();
        let normalized_sizes: Vec<f32> = if total_visible_size > 0.0 {
            visible_children_info.iter().map(|(_, s)| s / total_visible_size * 100.0).collect()
        } else {
            vec![100.0 / visible_children_info.len().max(1) as f32; visible_children_info.len()]
        };

        let mut elements: Vec<AnyElement> = Vec::new();

        for (visible_idx, (original_idx, _)) in visible_children_info.iter().enumerate() {
            let mut child_path = self.layout_path.clone();
            child_path.push(*original_idx);

            let container = self
                .child_containers
                .entry(child_path.clone())
                .or_insert_with(|| {
                    cx.new(|_cx| {
                        LayoutContainer::new(
                            self.workspace.clone(),
                            self.focus_manager.clone(),
                            self.request_broker.clone(),
                            self.window_id,
                            self.project_id.clone(),
                            self.project_path.clone(),
                            child_path.clone(),
                            self.backend.clone(),
                            self.terminals.clone(),
                            self.active_drag.clone(),
                            self.action_dispatcher.clone(),
                        )
                    })
                })
                .clone();

            if let Some(reg) = self.overlay_registry.clone() {
                container.update(cx, |c, _cx| c.set_overlay_registry(reg));
            }

            if visible_idx > 0 {
                let left_original_idx = visible_children_info[visible_idx - 1].0;
                let divider = render_split_divider(
                    self.workspace.clone(),
                    self.project_id.clone(),
                    left_original_idx,
                    *original_idx,
                    direction,
                    self.layout_path.clone(),
                    container_bounds_ref.clone(),
                    &self.active_drag,
                    self.action_dispatcher.clone(),
                    cx,
                );
                elements.push(divider.into_any_element());
            }

            let size_percent = normalized_sizes[visible_idx];
            // NOTE: Do NOT wrap the split child in `cached(size_full)`. GPUI's
            // `cached` stores the measured bounds and re-uses them across frames;
            // when a single terminal is split into two panes the child's previous
            // (full-window) width is cached and never re-measured against the new
            // `flex_basis`, so the right pane would render at full width and its
            // background bleed over the left pane. Let the `LayoutContainer` lay
            // out fresh each frame (its own root is `size_full`) so it honors the
            // split's flex size. The standalone-terminal `cached` calls keep their
            // wrapper because their parent is `flex_1` and doesn't change size.
            let child_element = div()
                .flex_basis(relative(size_percent / 100.0))
                .min_w_0()
                .min_h_0()
                .child(AnyView::from(container))
                .into_any_element();

            elements.push(child_element);
        }

        div()
            .id(ElementId::Name(format!("split-container-{}-{:?}", project_id, layout_path).into()))
            .child(canvas(
                {
                    let container_bounds_ref = container_bounds_ref.clone();
                    move |bounds, _window, _cx| {
                        *container_bounds_ref.borrow_mut() = bounds;
                    }
                },
                |_bounds, _prepaint, _window, _cx| {},
            ).absolute().size_full())
            .flex()
            .when(is_horizontal, |d| d.flex_col())
            .flex_nowrap()
            .size_full()
            .min_h_0()
            .min_w_0()
            .children(elements)
    }
}

impl<D: ActionDispatch + Send + Sync> Render for LayoutContainer<D> {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // One-time install of connection-status tracking: re-render this pane
        // on connect/disconnect events and via a short poll so per-tab status
        // dots (green = connected, red = disconnected) stay live.
        if !self.connection_init_done {
            self.connection_init_done = true;
            self.observe_connection_events(cx);
            self.start_connection_watch(cx);
        }

        if !self.background_cache_observed {
            if let Some(cache) = crate::terminal_background_cache(cx) {
                cx.observe(&cache, |_, _, cx| {
                    cx.notify();
                })
                .detach();
                self.background_cache_observed = true;
            }
        }

        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);
        let layout = self.get_layout(&self.workspace.read(cx)).cloned();

        match &layout {
            Some(LayoutNode::Terminal { .. }) => {
                if !self.child_containers.is_empty() {
                    self.deregister_child_resize_viewers_except(&HashSet::new(), cx);
                    self.child_containers.clear();
                }
            }
            Some(LayoutNode::Split { .. }) | Some(LayoutNode::Tabs { .. }) => {
                if let Some(pane) = self.terminal_pane.take() {
                    pane.update(cx, |pane, cx| pane.deregister_resize_viewer(cx));
                }
            }
            None => {
                self.deregister_resize_viewers(cx);
                self.terminal_pane = None;
                self.child_containers.clear();
            }
        }

        let content: AnyElement = match &layout {
            Some(LayoutNode::Terminal {
                terminal_id,
                minimized,
                detached,
                ..
            }) => self
                .render_terminal(terminal_id.clone(), *minimized, *detached, window, cx)
                .into_any_element(),

            Some(LayoutNode::Split {
                direction,
                sizes,
                children,
            }) => self
                .render_split(*direction, sizes, children, window, cx)
                .into_any_element(),

            Some(LayoutNode::Tabs {
                children,
                active_tab,
            }) => self
                .render_tabs(children, *active_tab, window, cx)
                .into_any_element(),

            None => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(p.text_muted)
                .child(i18n!(cx, "terminal.no_layout"))
                .into_any_element(),
        };

        div()
            .size_full()
            .relative()
            .child(content)
            .into_any_element()
    }
}
