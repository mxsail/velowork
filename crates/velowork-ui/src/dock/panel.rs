use crate::behavior::{HoverBehavior, StatefulElementBehaviorExt};
use crate::button::{Button, button_primary};
use crate::design::appearance::{ControlAppearance, ControlSize, ControlVariant};
use crate::design::semantic::SemanticPalette;
use crate::icon::AppIcon;
use crate::input::Input;
use crate::menu::{PopupMenu, PopupMenuDirection, PopupMenuItem};
use crate::overlay_registry::OverlayRegistry;
use crate::rename_state::{RenameState, cancel_rename, finish_rename, start_rename_with_blur};
use crate::tab::{tab_height, tab_style};
use crate::theme::{surface_bg, theme};
use crate::tokens::*;
use crate::{h_flex, v_flex};
use gpui::prelude::*;
use gpui::{
    Animation, AnimationExt, AnyElement, AnyView, App, AppContext, Bounds, Context, CursorStyle,
    DispatchPhase, ElementId, Entity, EventEmitter, FocusHandle, Focusable, HitboxBehavior,
    IntoElement, KeyDownEvent, MouseButton, MouseMoveEvent, MouseUpEvent, Pixels, Point, Render,
    SharedString, WeakEntity, Window, WindowControlArea, canvas, div, point, px, rgb, rgba,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use velowork_i18n::i18n;

use super::resize::ResizeHandle;
use super::types::{
    AccentColor, DockHeaderMode, PanelAction, PanelBadge, PanelCollapseState, PanelId, PanelInfo,
    PanelLifecycleEvent, PanelMode, PanelProvider, ResizeDragState, ResizeEdge as DockResizeEdge,
    ToolbarItem,
};

/// The drag payload for rearranging tabs.
#[allow(dead_code)]
#[derive(Clone)]
pub struct TabDragPayload {
    pub panel_id: PanelId,
    pub tab_index: usize,
}

/// The core Panel trait that business layers (Terminal, SFTP, etc.) must implement.
pub trait Panel: Render {
    /// Return metadata information about the panel.
    fn metadata(&self, cx: &App) -> PanelInfo;

    /// Lifecycle hooks:
    fn on_open(&mut self, _cx: &mut Context<Self>) {}
    fn on_close(&mut self, _cx: &mut Context<Self>) {}
    fn on_focus(&mut self, _cx: &mut Context<Self>) {}
    fn on_blur(&mut self, _cx: &mut Context<Self>) {}
    fn on_show(&mut self, _cx: &mut Context<Self>) {}
    fn on_hide(&mut self, _cx: &mut Context<Self>) {}
    fn on_resize(&mut self, _cx: &mut Context<Self>) {}
    fn on_tab_changed(&mut self, _cx: &mut Context<Self>) {}
    fn on_detach(&mut self, _cx: &mut Context<Self>) {}
    fn on_attach(&mut self, _cx: &mut Context<Self>) {}

    /// Custom header actions specific to this panel.
    fn custom_actions(&self, _cx: &App) -> Vec<PanelAction> {
        Vec::new()
    }

    /// Toolbar items rendered inline in the DockPanel header (right of the tab
    /// row). Each item is a self-contained data descriptor; the DockPanel knows
    /// how to render each variant. Panels override this to place rich controls
    /// (play/pause buttons, repeat/interval inputs, address bar, …) directly in
    /// the header area instead of consuming vertical space in the body.
    fn toolbar_elements(&self, _cx: &App) -> Vec<ToolbarItem> {
        Vec::new()
    }

    /// Status bar message specific to this panel.
    fn status_text(&self, _cx: &App) -> Option<String> {
        None
    }

    /// Focus handle for this panel, used to focus the panel when activated.
    fn focus_handle(&self, _cx: &App) -> Option<FocusHandle> {
        None
    }
}

/// A type-erased container wrapping `AnyView` and forwarding metadata and lifecycle callbacks.
#[derive(Clone)]
pub struct AnyPanel {
    pub view: AnyView,
    metadata_fn: Arc<dyn Fn(&App) -> PanelInfo + Send + Sync>,
    lifecycle_fn: Arc<dyn Fn(&AnyView, PanelLifecycleEvent, &mut App) + Send + Sync>,
    actions_fn: Arc<dyn Fn(&AnyView, &App) -> Vec<PanelAction> + Send + Sync>,
    toolbar_fn: Arc<dyn Fn(&AnyView, &App) -> Vec<ToolbarItem> + Send + Sync>,
    status_fn: Arc<dyn Fn(&AnyView, &App) -> Option<String> + Send + Sync>,
    focus_fn: Arc<dyn Fn(&AnyView, &App) -> Option<FocusHandle> + Send + Sync>,
}

impl AnyPanel {
    pub fn new<V>(view: Entity<V>) -> Self
    where
        V: Panel,
    {
        let view_clone = view.clone();
        let metadata_fn = Arc::new(move |cx: &App| view_clone.read(cx).metadata(cx));

        let _view_clone = view.clone();
        let lifecycle_fn = Arc::new(
            move |any_view: &AnyView, event: PanelLifecycleEvent, cx: &mut App| {
                if let Some(v) = any_view.clone().downcast::<V>().ok() {
                    let _ = v.update(cx, |this, cx| match event {
                        PanelLifecycleEvent::Open => this.on_open(cx),
                        PanelLifecycleEvent::Close => this.on_close(cx),
                        PanelLifecycleEvent::Focus => this.on_focus(cx),
                        PanelLifecycleEvent::Blur => this.on_blur(cx),
                        PanelLifecycleEvent::Show => this.on_show(cx),
                        PanelLifecycleEvent::Hide => this.on_hide(cx),
                        PanelLifecycleEvent::Resize => this.on_resize(cx),
                        PanelLifecycleEvent::TabChanged => this.on_tab_changed(cx),
                        PanelLifecycleEvent::Detach => this.on_detach(cx),
                        PanelLifecycleEvent::Attach => this.on_attach(cx),
                    });
                }
            },
        );

        let _view_clone = view.clone();
        let actions_fn = Arc::new(move |any_view: &AnyView, cx: &App| {
            if let Some(v) = any_view.clone().downcast::<V>().ok() {
                v.read(cx).custom_actions(cx)
            } else {
                Vec::new()
            }
        });

        let _view_clone = view.clone();
        let toolbar_fn = Arc::new(move |any_view: &AnyView, cx: &App| {
            if let Some(v) = any_view.clone().downcast::<V>().ok() {
                v.read(cx).toolbar_elements(cx)
            } else {
                Vec::new()
            }
        });

        let _view_clone = view.clone();
        let status_fn = Arc::new(move |any_view: &AnyView, cx: &App| {
            if let Some(v) = any_view.clone().downcast::<V>().ok() {
                v.read(cx).status_text(cx)
            } else {
                None
            }
        });

        let _view_clone = view.clone();
        let focus_fn = Arc::new(move |any_view: &AnyView, cx: &App| {
            if let Some(v) = any_view.clone().downcast::<V>().ok() {
                v.read(cx).focus_handle(cx)
            } else {
                None
            }
        });

        Self {
            view: view.into(),
            metadata_fn,
            lifecycle_fn,
            actions_fn,
            toolbar_fn,
            status_fn,
            focus_fn,
        }
    }

    pub fn metadata(&self, cx: &App) -> PanelInfo {
        (self.metadata_fn)(cx)
    }

    pub fn trigger_lifecycle(&self, event: PanelLifecycleEvent, cx: &mut App) {
        (self.lifecycle_fn)(&self.view, event, cx);
    }

    pub fn custom_actions(&self, cx: &App) -> Vec<PanelAction> {
        (self.actions_fn)(&self.view, cx)
    }

    /// Toolbar items that the active panel wants rendered in the header.
    pub fn toolbar_elements(&self, cx: &App) -> Vec<ToolbarItem> {
        (self.toolbar_fn)(&self.view, cx)
    }

    pub fn status_text(&self, cx: &App) -> Option<String> {
        (self.status_fn)(&self.view, cx)
    }

    pub fn focus_handle(&self, cx: &App) -> Option<FocusHandle> {
        (self.focus_fn)(&self.view, cx)
    }
}

pub struct DockPanelEvent;

pub struct DockPanelDetachEvent {
    pub id: String,
}

/// Emitted when the whole panel (every tab) is detached into its own OS window.
/// Carries the actual tab entities (cloned `AnyPanel`s share the underlying
/// panel entities, so each panel's internal state is preserved across the
/// detach) plus the previously-active tab index so the detached window can
/// restore the same selection.
pub struct DockPanelDetachWholeEvent {
    pub tabs: Vec<AnyPanel>,
    pub active_tab_index: Option<usize>,
}

/// Emitted when the user picks "hide panel" from the dock's right-click menu.
///
/// The `DockPanel` itself does NOT know how it is embedded in the surrounding
/// layout, so it cannot make the whole dock region disappear on its own
/// (setting its internal `collapse_state` only collapses the panel *inside* its
/// container, leaving an empty gap). The app layer owns the sidebar controller
/// that actually removes the region, so it listens for this event and hides the
/// dock the same way the status-bar "toggle right sidebar" button does.
pub struct DockPanelHideEvent;

impl crate::overlay::CloseEvent for DockPanelHideEvent {
    fn is_close(&self) -> bool {
        true
    }
}

/// Identifies a hover-animated element so multiple elements can share the
/// single progress-driven animation engine without their state colliding.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum HoverChannel {
    /// A tab's close button (red background tint fade).
    #[allow(dead_code)]
    Close(usize),
    /// A tab header (selected-state background fade).
    Tab(usize),
}

/// In-flight hover animation segment for a single channel.
#[derive(Clone, Copy)]
struct HoverAnim {
    /// Progress (0..1) at the moment this segment started.
    from: f32,
    /// Progress (0..1) this segment animates toward.
    to: f32,
    /// Wall-clock time the segment started.
    start: Instant,
}

pub struct DockPanel {
    pub id: ElementId,
    pub tabs: Vec<AnyPanel>,
    pub active_tab_index: Option<usize>,
    pub collapse_state: PanelCollapseState,
    pub header_mode: DockHeaderMode,

    /// When true, tab icons are rendered in the tab row; when false only the
    /// title (and badge) are shown. Toggled from the tab-row context menu.
    pub show_icons: bool,
    pub mode: PanelMode,
    pub size: f32,
    pub resize_edge: Option<DockResizeEdge>,
    /// When true the panel fills its parent (e.g. a detached OS window) in both
    /// width and height instead of being sized by `size`/`resize_edge`.
    pub fill_window: bool,

    // Drag resizing state
    pub resize_dragging: Option<ResizeDragState>,

    /// Transient, render-only cap on the panel's effective width/height. Set by
    /// the `DockManager` each frame so the panel never overflows the available
    /// dock space (e.g. when the hardcoded `size` clamp exceeds the window
    /// width). Not persisted — `size` remains the user-controlled value.
    pub max_render_size: Option<f32>,

    /// Corner radius (px) the panel should use when it sits on a window corner
    /// (custom titlebar with rounded corners). Injected by the app each frame,
    /// like `max_render_size`, so this UI crate stays independent of the
    /// settings system. `None` falls back to the legacy 8px.
    pub window_corner_radius: Option<f32>,

    // Rename state for tab title inline editing
    pub rename_state: Option<RenameState<PanelId>>,
    pub title_overrides: HashMap<PanelId, String>,

    // More menu / Context menu dropdown overlay state
    pub active_menu: Option<Entity<PopupMenu>>,
    pub menu_toggle_guard: Option<std::time::Instant>,
    pub more_button_bounds: Bounds<Pixels>,

    // Layout changed callback
    pub on_layout_changed: Option<Arc<dyn Fn(&mut Window, &mut App) + Send + Sync>>,

    // Attach to main window callback (for detached floating panels)
    pub on_attach: Option<Arc<dyn Fn(&str, &mut Window, &mut App) + Send + Sync>>,

    /// Panels that can be toggled on/off from this dock's "more" menu.
    /// Only metadata is stored; the concrete panel is produced on demand
    /// by `on_request_panel`.
    pub panel_providers: Vec<PanelProvider>,

    /// Produces a panel instance for the given provider id. Supplied by the
    /// app layer so `DockPanel` stays decoupled from concrete panels.
    pub on_request_panel:
        Option<Arc<dyn Fn(&str, &mut Window, &mut App) -> AnyPanel + Send + Sync>>,

    /// Other docks that must not hold the same panel simultaneously.
    /// When a panel is added here it is first removed from every peer,
    /// enforcing the "a panel lives in exactly one dock" rule.
    pub peer_docks: Vec<WeakEntity<DockPanel>>,

    /// When true the OS-window close handler must NOT re-attach the panel
    /// (used when the panel was pulled out by the single-dock rule).
    pub suppress_reattach: bool,

    /// Registry for centralized click-outside dismissal of this dock's
    /// menus. `None` keeps the legacy local-backdrop behavior.
    pub overlay_registry: Option<Entity<OverlayRegistry>>,
    pub header_measured_width: Option<f32>,

    /// Per-element hover animation state, keyed by `HoverChannel`. `progress`
    /// is the currently displayed fade amount (0..1); `target` is where it is
    /// heading; `anim` holds the active animation segment. Drives smooth hover
    /// feedback (red close-button tint, selected-state tab background) without
    /// touching layout/size.
    hover_progress: HashMap<HoverChannel, f32>,
    hover_target: HashMap<HoverChannel, f32>,
    hover_anim: HashMap<HoverChannel, HoverAnim>,

    focus_handle: FocusHandle,
}

impl EventEmitter<DockPanelEvent> for DockPanel {}
impl EventEmitter<DockPanelDetachEvent> for DockPanel {}
impl EventEmitter<DockPanelDetachWholeEvent> for DockPanel {}
impl EventEmitter<DockPanelHideEvent> for DockPanel {}

impl DockPanel {
    pub fn new(id: impl Into<ElementId>, size: f32, cx: &mut Context<Self>) -> Self {
        Self {
            id: id.into(),
            tabs: Vec::new(),
            active_tab_index: None,
            collapse_state: PanelCollapseState::Normal,
            header_mode: DockHeaderMode::Auto,
            show_icons: true,
            mode: PanelMode::Normal,
            size,
            resize_edge: None,
            fill_window: false,
            resize_dragging: None,
            max_render_size: None,
            window_corner_radius: None,
            rename_state: None,
            title_overrides: HashMap::new(),
            active_menu: None,
            menu_toggle_guard: None,
            more_button_bounds: Bounds::default(),
            on_layout_changed: None,
            on_attach: None,
            panel_providers: Vec::new(),
            on_request_panel: None,
            peer_docks: Vec::new(),
            suppress_reattach: false,
            overlay_registry: None,
            header_measured_width: None,
            hover_progress: HashMap::new(),
            hover_target: HashMap::new(),
            hover_anim: HashMap::new(),
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn set_header_mode(&mut self, mode: DockHeaderMode) {
        self.header_mode = mode;
    }

    pub fn set_on_attach(
        &mut self,
        cb: impl Fn(&str, &mut Window, &mut App) + Send + Sync + 'static,
    ) {
        self.on_attach = Some(Arc::new(cb));
    }

    /// Register the panels that can be toggled from this dock's "more" menu.
    pub fn set_panel_providers(&mut self, providers: Vec<PanelProvider>) {
        self.panel_providers = providers;
    }

    /// Set the callback that creates a panel instance for a given provider id.
    pub fn set_on_request_panel(
        &mut self,
        cb: impl Fn(&str, &mut Window, &mut App) -> AnyPanel + Send + Sync + 'static,
    ) {
        self.on_request_panel = Some(Arc::new(cb));
    }

    /// Register sibling docks that must not hold the same panel simultaneously.
    pub fn set_peer_docks(&mut self, peers: Vec<WeakEntity<DockPanel>>) {
        self.peer_docks = peers;
    }

    /// Set the overlay registry this dock's menus register with for
    /// centralized click-outside dismissal. `None` keeps the
    /// legacy local-backdrop behavior.
    pub fn set_overlay_registry(&mut self, reg: Entity<OverlayRegistry>) {
        self.overlay_registry = Some(reg);
    }

    /// Add a panel (by provider id) to this dock, enforcing the single-dock
    /// rule: the panel is first removed from every peer dock. Returns `true`
    /// if the panel was (re)attached to this dock.
    pub fn add_panel_by_id(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        // Enforce: a panel can live in exactly one dock. Pull it out of
        // any peer that currently holds it.
        for peer in &self.peer_docks {
            if let Some(peer) = peer.upgrade() {
                let _ = peer.update(cx, |p, cx| {
                    if let Some(idx) = p.tabs.iter().position(|t| t.metadata(cx).id.0 == id) {
                        // Mark so the peer's OS-window close handler (if any)
                        // does not re-attach the panel back here.
                        p.suppress_reattach = true;
                        p.close_tab(idx, window, cx);
                    }
                });
            }
        }

        if self.tabs.iter().any(|t| t.metadata(cx).id.0 == id) {
            return false;
        }

        let factory = match &self.on_request_panel {
            Some(f) => f.clone(),
            None => return false,
        };
        let panel = factory(id, window, cx);
        self.add_tab(panel, cx);
        true
    }

    /// Activate tab by id if already present, otherwise construct and add it via `on_request_panel`.
    pub fn activate_or_add_panel(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.select_tab_by_id(id, cx) {
            true
        } else {
            let added = self.add_panel_by_id(id, window, cx);
            if added {
                self.select_tab_by_id(id, cx);
            }
            added
        }
    }

    /// Remove a panel (by id) from this dock if present.
    pub fn remove_panel_by_id(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(idx) = self.tabs.iter().position(|t| t.metadata(cx).id.0 == id) {
            self.close_tab(idx, window, cx);
        }
    }

    pub fn set_resize_edge(&mut self, edge: DockResizeEdge, cx: &mut Context<Self>) {
        self.resize_edge = Some(edge);
        cx.emit(DockPanelEvent);
        cx.notify();
    }

    pub fn active_tab(&self) -> Option<&AnyPanel> {
        self.active_tab_index.and_then(|idx| self.tabs.get(idx))
    }

    pub fn start_rename(&mut self, tab_idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.tabs.get(tab_idx) {
            let meta = panel.metadata(cx);
            if meta.closable {
                let panel_id = meta.id.clone();
                let rename_state = start_rename_with_blur(
                    panel_id.clone(),
                    &meta.title,
                    "Panel name...",
                    move |this, window, cx| {
                        this.commit_rename(window, cx);
                    },
                    window,
                    cx,
                );
                self.rename_state = Some(rename_state);
                cx.notify();
            }
        }
    }

    fn commit_rename(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some((panel_id, new_name)) = finish_rename(&mut self.rename_state, cx) {
            if let Some(panel_idx) = self.tabs.iter().position(|p| p.metadata(cx).id == panel_id) {
                self.title_overrides
                    .insert(panel_id.clone(), new_name.clone());
                self.tabs[panel_idx].trigger_lifecycle(PanelLifecycleEvent::TabChanged, cx);
                cx.emit(DockPanelEvent);
            }
        }
        cx.notify();
    }

    pub fn add_tab(&mut self, panel: AnyPanel, cx: &mut Context<Self>) {
        if self.collapse_state != PanelCollapseState::Normal {
            self.collapse_state = PanelCollapseState::Normal;
        }
        panel.trigger_lifecycle(PanelLifecycleEvent::Open, cx);
        self.tabs.push(panel);
        if self.active_tab_index.is_none() {
            self.active_tab_index = Some(0);
            if let Some(p) = self.tabs.first() {
                p.trigger_lifecycle(PanelLifecycleEvent::Focus, cx);
            }
        }
        cx.emit(DockPanelEvent);
        cx.notify();
    }

    pub fn close_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }
        let panel = self.tabs.remove(index);
        panel.trigger_lifecycle(PanelLifecycleEvent::Close, cx);

        // Re-key hover state so the removed tab's channels are dropped and the
        // surviving tabs keep their hover on the shifted index (no stale hover
        // background when a new tab later reuses an old index).
        Self::reindex_hover_map(&mut self.hover_progress, index);
        Self::reindex_hover_map(&mut self.hover_target, index);
        Self::reindex_hover_map(&mut self.hover_anim, index);

        if self.tabs.is_empty() {
            self.active_tab_index = None;
            if self.fill_window {
                // 独立 OS 窗口中的最后一个标签被关闭：直接关闭整个窗口，
                // 而不是留下一个空白的独占窗口。
                window.remove_window();
                return;
            }
            // 非独立窗口：关闭最后一个标签等价于触发"隐藏面板"，直接复用
            // 各 dock 已有的隐藏按钮逻辑（含右侧清除高亮、底部动画收起、
            // 左侧收起等），避免在 close_tab 里散落各 dock 的收起处理。
            cx.emit(DockPanelHideEvent);
            cx.notify();
            return;
        } else if let Some(active) = self.active_tab_index {
            if active >= self.tabs.len() {
                self.active_tab_index = Some(self.tabs.len() - 1);
            } else if active == index {
                self.active_tab_index = Some(active);
            } else if active > index {
                self.active_tab_index = Some(active - 1);
            }
        }

        if let Some(idx) = self.active_tab_index {
            self.tabs[idx].trigger_lifecycle(PanelLifecycleEvent::TabChanged, cx);
        }

        cx.emit(DockPanelEvent);
        cx.notify();
    }

    pub fn select_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.tabs.len() {
            // Auto-expand from collapsed/hidden so the content area becomes visible
            // immediately when the user selects a tab.
            if self.collapse_state != PanelCollapseState::Normal {
                self.collapse_state = PanelCollapseState::Normal;
            }
            if let Some(old_idx) = self.active_tab_index {
                if old_idx != index {
                    self.tabs[old_idx].trigger_lifecycle(PanelLifecycleEvent::Blur, cx);
                }
            }
            self.active_tab_index = Some(index);
            self.tabs[index].trigger_lifecycle(PanelLifecycleEvent::Focus, cx);
            self.tabs[index].trigger_lifecycle(PanelLifecycleEvent::TabChanged, cx);
            cx.emit(DockPanelEvent);
            cx.notify();
        }
    }

    pub fn select_tab_by_id(&mut self, id: &str, cx: &mut Context<Self>) -> bool {
        if let Some(idx) = self.tabs.iter().position(|tab| tab.metadata(cx).id.0 == id) {
            self.select_tab(idx, cx);
            true
        } else {
            false
        }
    }

    pub fn active_tab_id(&self, cx: &App) -> Option<String> {
        self.active_tab().map(|tab| tab.metadata(cx).id.0.clone())
    }

    pub fn is_focused(&self, window: &Window, cx: &App) -> bool {
        if let Some(tab) = self.active_tab() {
            if let Some(focus_handle) = tab.focus_handle(cx) {
                if focus_handle.contains_focused(window, cx) {
                    return true;
                }
            }
        }
        self.focus_handle.contains_focused(window, cx)
    }

    pub fn focus_active_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_tab() {
            if let Some(focus_handle) = tab.focus_handle(cx) {
                window.focus(&focus_handle, cx);
                return;
            }
        }
        window.focus(&self.focus_handle, cx);
    }

    pub fn select_next_tab(&mut self, cx: &mut Context<Self>) {
        if self.tabs.is_empty() {
            return;
        }
        let current = self.active_tab_index.unwrap_or(0);
        let next = (current + 1) % self.tabs.len();
        self.select_tab(next, cx);
    }

    pub fn select_prev_tab(&mut self, cx: &mut Context<Self>) {
        if self.tabs.is_empty() {
            return;
        }
        let current = self.active_tab_index.unwrap_or(0);
        let prev = if current == 0 {
            self.tabs.len() - 1
        } else {
            current - 1
        };
        self.select_tab(prev, cx);
    }

    pub fn close_other_tabs(
        &mut self,
        keep_idx: usize,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if keep_idx >= self.tabs.len() {
            return;
        }
        let panel_to_keep = self.tabs[keep_idx].clone();
        for (i, panel) in self.tabs.iter().enumerate() {
            if i != keep_idx {
                panel.trigger_lifecycle(PanelLifecycleEvent::Close, cx);
            }
        }
        self.tabs = vec![panel_to_keep];
        self.active_tab_index = Some(0);
        // All but the kept tab (now the only one at index 0) are gone; drop the
        // hover channels of every other index to avoid stale hover backgrounds.
        Self::clear_hover_from(&mut self.hover_progress, 1);
        Self::clear_hover_from(&mut self.hover_target, 1);
        Self::clear_hover_from(&mut self.hover_anim, 1);
        cx.emit(DockPanelEvent);
        cx.notify();
    }

    pub fn close_tabs_to_right(
        &mut self,
        index: usize,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if index >= self.tabs.len() {
            return;
        }
        let to_remove: Vec<_> = self.tabs.drain((index + 1)..).collect();
        for panel in to_remove {
            panel.trigger_lifecycle(PanelLifecycleEvent::Close, cx);
        }
        // Tabs at indices > `index` are gone; drop their hover channels so a
        // later tab reusing those indices doesn't inherit a stale hover.
        Self::clear_hover_from(&mut self.hover_progress, index + 1);
        Self::clear_hover_from(&mut self.hover_target, index + 1);
        Self::clear_hover_from(&mut self.hover_anim, index + 1);
        if let Some(active) = self.active_tab_index {
            if active > index {
                self.active_tab_index = Some(index);
            }
        }
        cx.emit(DockPanelEvent);
        cx.notify();
    }

    /// Remove a tab by its business panel id (no window needed).
    /// Does nothing if the id isn't found.
    pub fn remove_tab_by_id(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(index) = self.tabs.iter().position(|t| t.metadata(cx).id.0 == id) else {
            return;
        };
        let panel = self.tabs.remove(index);
        panel.trigger_lifecycle(PanelLifecycleEvent::Close, cx);

        Self::reindex_hover_map(&mut self.hover_progress, index);
        Self::reindex_hover_map(&mut self.hover_target, index);
        Self::reindex_hover_map(&mut self.hover_anim, index);

        if self.tabs.is_empty() {
            self.active_tab_index = None;
        } else if let Some(active) = self.active_tab_index {
            if active >= self.tabs.len() {
                self.active_tab_index = Some(self.tabs.len() - 1);
            } else if active == index {
                self.active_tab_index = Some(active);
            } else if active > index {
                self.active_tab_index = Some(active - 1);
            }
        }
        cx.emit(DockPanelEvent);
        cx.notify();
    }

    pub fn detach_tab(&mut self, index: usize, _window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }
        let panel = self.tabs[index].clone();
        let metadata = panel.metadata(cx);
        panel.trigger_lifecycle(PanelLifecycleEvent::Detach, cx);
        self.close_tab(index, _window, cx);
        cx.emit(DockPanelDetachEvent { id: metadata.id.0 });
    }

    /// Detach the entire panel (every tab) into its own OS window. Unlike
    /// `detach_tab`, which pulls a single tab out, this moves all tabs at once.
    /// The cloned `AnyPanel`s share the underlying panel entities, so each
    /// panel's internal state survives the move; the source dock is left empty
    /// until the detached window is re-attached (on close or via "attach").
    pub fn detach_panel(&mut self, cx: &mut Context<Self>) {
        // Detaching is a panel-level action: it is valid even when the panel
        // currently holds no tabs (the detached window opens empty, and its
        // "more" menu panel picker lets the user add panels there). We do NOT
        // early-return on empty tabs, so an empty panel can still be detached.
        //
        // Snapshot the tabs (sharing the same underlying entities) before
        // clearing the source dock, so the detached window can rebuild the
        // full panel with identical state and tab order.
        let tabs = self.tabs.clone();
        let active = self.active_tab_index;
        for panel in &self.tabs {
            panel.trigger_lifecycle(PanelLifecycleEvent::Detach, cx);
        }
        self.tabs.clear();
        self.active_tab_index = None;
        // Drop the maximized / fullscreen mode. After detaching the whole panel
        // the source dock becomes empty and is hidden in the main window; if its
        // `mode` stayed `Maximized`, `is_bottom_dock_fullscreen()` would still
        // report `true`, hiding the main window's projects grid and status bar
        // while the (now empty) dock renders nothing — leaving the main window
        // completely blank. Resetting to `Normal` lets the main window restore
        // its layout once the dock is hidden.
        self.mode = PanelMode::Normal;
        // The dock is now empty; drop all hover state so a tab later re-added at
        // index 0 (e.g. on re-attach) starts with a clean, non-hovered close button.
        self.hover_progress.clear();
        self.hover_target.clear();
        self.hover_anim.clear();
        cx.emit(DockPanelDetachWholeEvent {
            tabs,
            active_tab_index: active,
        });
        cx.notify();
    }

    /// Dynamically calculate the minimum size required by this panel content
    pub fn required_min_size(&self, cx: &App) -> f32 {
        self.min_header_size(cx)
    }

    /// Dynamically calculate the minimum width required by this panel so that
    /// at least one full active tab (icon + title text + close button)
    /// or title text (with at least 2 characters + ellipsis) and all
    /// header toolbar action buttons remain completely visible.
    pub fn min_header_size(&self, cx: &App) -> f32 {
        let is_title_mode = match self.header_mode {
            DockHeaderMode::TitleOnly => true,
            DockHeaderMode::Tabs => false,
            DockHeaderMode::Auto => self.tabs.len() <= 1,
        };

        // 1. Calculate header toolbar actions width (Right-side items)
        let scale = crate::tokens::ui_scale_factor(cx);
        let action_btn_size = f32::from(crate::design::appearance::control_height_for_size(ControlSize::Compact, cx));
        let action_gap = f32::from(crate::tokens::ui_space_sm(cx));
        let divider_w = 9.0;
        let mut actions_w = action_btn_size + divider_w; // Base: "More" menu + Divider
        let mut action_count = 2; // more + divider
        if let Some(active_panel) = self.active_tab() {
            let custom_count = active_panel.custom_actions(cx).len();
            actions_w += custom_count as f32 * action_btn_size;
            action_count += custom_count;
        }
        if !self.fill_window {
            actions_w += action_btn_size; // Expand / Fullscreen button
            action_count += 1;
        }
        if self.mode != PanelMode::Maximized && !self.fill_window {
            actions_w += action_btn_size; // Close / Hide button
            action_count += 1;
        }
        if self.fill_window {
            actions_w += 80.0 * scale; // Detached window CSD buttons
            action_count += 1;
        }
        let action_gaps_w = action_count.saturating_sub(1) as f32 * action_gap;
        let actions_total_w = actions_w + action_gaps_w;

        // 2. Calculate inline toolbar items width
        let toolbar_items: Vec<ToolbarItem> = self
            .active_tab()
            .map(|p| p.toolbar_elements(cx))
            .unwrap_or_default();
        let mut toolbar_w = 0.0;
        let mut toolbar_count: usize = 0;
        for item in &toolbar_items {
            toolbar_w += item.estimated_width();
            toolbar_count += 1;
        }
        if toolbar_count > 0 {
            toolbar_w += toolbar_count.saturating_sub(1) as f32 * 6.0 + 12.0;
        }

        // 3. Calculate minimum content width (icon + at least 2 chars + ellipsis + paddings)
        let icon_w = if self.show_icons { 24.0 } else { 0.0 };
        let text_min_w = 48.0; // 2 Chinese chars (~28px) + "..." (~15px) + gap (~5px)
        let padding_w = 30.0; // Left (12px) + gap to actions (6px) + Right (12px)

        let content_min = if is_title_mode {
            icon_w + text_min_w + padding_w
        } else {
            let is_closable = self.active_tab().map(|t| t.metadata(cx).closable).unwrap_or(false);
            let close_w = if is_closable { 20.0 } else { 0.0 };
            icon_w + text_min_w + close_w + padding_w + 16.0
        };

        let calculated_min = content_min + actions_total_w + toolbar_w;
        calculated_min.clamp(170.0, 500.0)
    }

/// Truncate text using standard bottom three dots (`"..."`) when it exceeds available width,
/// ensuring at least 2 characters are preserved before ellipsis.
pub fn truncate_with_bottom_dots(text: &str, avail_w: f32) -> String {
    let char_count = text.chars().count();
    if char_count <= 2 {
        return text.to_string();
    }

    let mut total_w = 0.0;
    let mut char_widths = Vec::with_capacity(char_count);
    for ch in text.chars() {
        let w = if ch.is_ascii() { 7.5 } else { 14.0 };
        char_widths.push(w);
        total_w += w;
    }

    if total_w <= avail_w {
        return text.to_string();
    }

    let dots_w = 15.0; // "..." width
    let budget = (avail_w - dots_w).max(0.0);

    let mut accum_w = 0.0;
    let mut take_chars = 0;
    for (idx, &w) in char_widths.iter().enumerate() {
        if accum_w + w <= budget {
            accum_w += w;
            take_chars = idx + 1;
        } else {
            break;
        }
    }

    let take_chars = take_chars.max(2).min(char_count);
    if take_chars >= char_count {
        text.to_string()
    } else {
        let prefix: String = text.chars().take(take_chars).collect();
        format!("{prefix}...")
    }
}

    fn handle_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let cmd_or_ctrl = event.keystroke.modifiers.platform || event.keystroke.modifiers.control;
        if cmd_or_ctrl && event.keystroke.key == "w" {
            if let Some(idx) = self.active_tab_index {
                self.close_tab(idx, window, cx);
                cx.stop_propagation();
            }
        } else if cmd_or_ctrl && event.keystroke.key == "Tab" {
            if event.keystroke.modifiers.shift {
                self.select_prev_tab(cx);
            } else {
                self.select_next_tab(cx);
            }
            cx.stop_propagation();
        } else if cmd_or_ctrl && event.keystroke.key.as_str() == "f" {
            self.focus_active_tab(window, cx);
        }
    }

    /// Show the overflow tabs dropdown menu when there are more tabs than header space
    pub fn show_overflow_tabs_menu(
        &mut self,
        overflow_indices: Vec<usize>,
        button_pos: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(ctx) = self.active_menu.take() {
            ctx.update(cx, |m, cx| {
                m.set_on_close(None);
                m.close(window, cx);
            });
        }

        let mut menu_items = Vec::new();
        let this_weak = cx.entity().downgrade();

        for idx in overflow_indices {
            if let Some(panel) = self.tabs.get(idx) {
                let meta = panel.metadata(cx);
                let title = self
                    .title_overrides
                    .get(&meta.id)
                    .cloned()
                    .unwrap_or(meta.title);
                let icon = meta.icon;
                let is_active = self.active_tab_index == Some(idx);
                let this_weak_select = this_weak.clone();

                let mut item = PopupMenuItem::item(
                    format!("overflow-tab-{}", idx),
                    title,
                    move |window, cx| {
                        if let Some(this) = this_weak_select.upgrade() {
                            let _ = this.update(cx, |this, cx| {
                                this.select_tab(idx, cx);
                                this.focus_active_tab(window, cx);
                            });
                        }
                    },
                )
                .icon(icon);

                if is_active {
                    item = item.checked(true);
                }

                menu_items.push(item);
            }
        }

        let this_weak_close = this_weak.clone();
        let reg = self.overlay_registry.clone();
        let menu = cx.new(move |cx| PopupMenu::new(cx, menu_items, button_pos, reg, None));

        let menu_id = menu.entity_id();
        let on_close: Arc<dyn Fn(&mut Window, &mut App) + Send + Sync> = Arc::new(move |_, cx| {
            if let Some(this) = this_weak_close.upgrade() {
                let _ = this.update(cx, |this, cx| {
                    if this.active_menu.as_ref().map(|m| m.entity_id()) == Some(menu_id) {
                        this.active_menu = None;
                        cx.notify();
                    }
                });
            }
        });
        menu.update(cx, |m, _| m.set_on_close(Some(on_close)));

        let focus_handle = menu.read(cx).focus_handle.clone();
        window.focus(&focus_handle, cx);
        self.active_menu = Some(menu);
        cx.notify();
    }

    /// Unified right-click menu for the tab row and the tabs themselves.
    ///
    /// `tab_index` carries the click context:
    /// - `Some(i)` — the right-click landed on a specific tab, so the menu
    ///   shows the shared options followed by the tab-specific actions
    ///   (rename, detach, close).
    /// - `None` — the click was on the empty tab row, so only the shared
    ///   options are shown (panel picker, icon visibility, hide/show panel).
    ///
    /// This merges the former `show_tab_context_menu` (tab-specific) and
    /// `show_tabs_context_menu` (tab-row) into a single, context-aware menu
    /// with no duplicated entries.
    fn show_context_menu(
        &mut self,
        tab_index: Option<usize>,
        click_pos: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(ctx) = self.active_menu.take() {
            ctx.update(cx, |m, cx| {
                m.set_on_close(None);
                m.close(window, cx);
            });
        }

        let mut menu_items = Vec::new();
        let this_weak = cx.entity().downgrade();

        let this_weak_icons = this_weak.clone();
        let show_icons = self.show_icons;
        menu_items.push(
            PopupMenuItem::item(
                "toggle-icons",
                i18n!(cx, "dock.tab.show_icons"),
                move |_window, cx| {
                    if let Some(this) = this_weak_icons.upgrade() {
                        let _ = this.update(cx, |this, cx| {
                            this.show_icons = !this.show_icons;
                            cx.notify();
                        });
                    }
                },
            )
            .icon(AppIcon::Eye)
            .checked(show_icons),
        );

        let this_weak_hide = this_weak.clone();
        menu_items.push(
            PopupMenuItem::item(
                "hide-panel",
                i18n!(cx, "dock.panel.hide"),
                move |_window, cx| {
                    if let Some(this) = this_weak_hide.upgrade() {
                        let _ = this.update(cx, |_this, cx| {
                            cx.emit(DockPanelHideEvent);
                        });
                    }
                },
            )
            .icon(AppIcon::Minimize),
        );

        if let Some(index) = tab_index {
            menu_items.push(PopupMenuItem::separator());

            let this_weak_clone = this_weak.clone();
            menu_items.push(
                PopupMenuItem::item(
                    "rename",
                    i18n!(cx, "dock.panel.rename"),
                    move |window, cx| {
                        if let Some(this) = this_weak_clone.upgrade() {
                            let _ = this.update(cx, |this, cx| {
                                this.start_rename(index, window, cx);
                            });
                        }
                    },
                )
                .icon(AppIcon::Edit),
            );

            let this_weak_clone = this_weak.clone();
            menu_items.push(
                PopupMenuItem::item(
                    "detach",
                    i18n!(cx, "dock.panel.detach"),
                    move |window, cx| {
                        if let Some(this) = this_weak_clone.upgrade() {
                            let _ = this.update(cx, |this, cx| {
                                this.detach_tab(index, window, cx);
                            });
                        }
                    },
                )
                .icon(AppIcon::Detach),
            );

            let is_closable = self.tabs.get(index).map(|t| t.metadata(cx).closable).unwrap_or(true);
            if is_closable {
                let this_weak_clone = this_weak.clone();
                menu_items.push(
                    PopupMenuItem::item("close", i18n!(cx, "dock.tab.close"), move |window, cx| {
                        if let Some(this) = this_weak_clone.upgrade() {
                            let _ = this.update(cx, |this, cx| {
                                this.close_tab(index, window, cx);
                            });
                        }
                    })
                    .icon(AppIcon::Close)
                    .shortcut("Ctrl+W"),
                );
            }
        }

        let this_weak_close = this_weak.clone();
        let reg = self.overlay_registry.clone();
        let menu = cx.new(move |cx| PopupMenu::new(cx, menu_items, click_pos, reg, None));

        let menu_id = menu.entity_id();
        let on_close: Arc<dyn Fn(&mut Window, &mut App) + Send + Sync> = Arc::new(move |_, cx| {
            if let Some(this) = this_weak_close.upgrade() {
                let _ = this.update(cx, |this, cx| {
                    if this.active_menu.as_ref().map(|m| m.entity_id()) == Some(menu_id) {
                        this.active_menu = None;
                        cx.notify();
                    }
                });
            }
        });
        menu.update(cx, |m, _| m.set_on_close(Some(on_close)));

        let focus_handle = menu.read(cx).focus_handle.clone();
        window.focus(&focus_handle, cx);
        self.active_menu = Some(menu);
        cx.notify();
    }

    /// Quadratic ease-in-out: slow start/end, faster middle — matches the
    /// project's `--transition-fast` feel for hover feedback.
    fn ease_in_out(t: f32) -> f32 {
        if t < 0.5 {
            2.0 * t * t
        } else {
            let x = -2.0 * t + 2.0;
            1.0 - x * x / 2.0
        }
    }

    /// Begin (or redirect) the hover fade for `channel`.
    /// `hovered == true` fades the element's hover style in; `false` fades it
    /// out. Each channel animates independently so a tab and its close button
    /// never interfere.
    fn set_hover(
        &mut self,
        channel: HoverChannel,
        hovered: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = if hovered { 1.0 } else { 0.0 };
        let current = self.hover_progress.get(&channel).copied().unwrap_or(0.0);
        let already_running = self.hover_anim.contains_key(&channel);
        self.hover_target.insert(channel, target);
        self.hover_anim.insert(
            channel,
            HoverAnim {
                from: current,
                to: target,
                start: Instant::now(),
            },
        );
        if !already_running {
            self.tick_hover(channel, window, cx);
        }
    }

    /// Advance one frame of a hover fade, then schedule the next frame until
    /// the displayed progress reaches the target.
    fn tick_hover(&mut self, channel: HoverChannel, window: &mut Window, cx: &mut Context<Self>) {
        const DURATION: Duration = Duration::from_millis(150);
        let (from, to, start) = match self.hover_anim.get(&channel) {
            Some(a) => (a.from, a.to, a.start),
            None => return,
        };
        let elapsed = start.elapsed().as_secs_f32();
        let t = (elapsed / DURATION.as_secs_f32()).clamp(0.0, 1.0);
        let progress = from + (to - from) * Self::ease_in_out(t);
        self.hover_progress.insert(channel, progress);

        let target = self.hover_target.get(&channel).copied().unwrap_or(0.0);
        if t < 1.0 || (to - target).abs() >= f32::EPSILON {
            // Still animating, or the target changed mid-flight — keep going.
            let weak = cx.entity().downgrade();
            window.on_next_frame(move |window, cx| {
                if let Some(entity) = weak.upgrade() {
                    let _ = entity.update(cx, |this, cx| {
                        this.tick_hover(channel, window, cx);
                    });
                }
            });
        } else {
            self.hover_progress.insert(channel, to);
            self.hover_anim.remove(&channel);
        }
        cx.notify();
    }

    /// Re-key a hover map after the tab at `removed` has been taken out of
    /// `self.tabs` and the later tabs have shifted down by one index.
    ///
    /// Without this, the removed tab's `HoverChannel::{Tab,Close}(removed)`
    /// entries linger at progress ~1.0 (the `on_hover(false)` callback never
    /// fires because the element is gone), and a later tab reusing that index
    /// — e.g. the first tab added to an emptied panel — would inherit a stale
    /// hover background. Surviving tabs keep their hover state on the new index.
    fn reindex_hover_map<V>(map: &mut HashMap<HoverChannel, V>, removed: usize) {
        let taken = std::mem::take(map);
        *map = taken
            .into_iter()
            .filter_map(|(ch, v)| match ch {
                HoverChannel::Tab(i) if i == removed => None,
                HoverChannel::Close(i) if i == removed => None,
                HoverChannel::Tab(i) if i > removed => Some((HoverChannel::Tab(i - 1), v)),
                HoverChannel::Close(i) if i > removed => Some((HoverChannel::Close(i - 1), v)),
                other => Some((other, v)),
            })
            .collect();
    }

    /// Drop every hover channel whose index is `>= start`. Used by bulk-removal
    /// operations (close others / close to the right) where many tabs vanish at
    /// once and per-index re-keying would be ambiguous.
    fn clear_hover_from<V>(map: &mut HashMap<HoverChannel, V>, start: usize) {
        map.retain(|ch, _| match ch {
            HoverChannel::Tab(i) | HoverChannel::Close(i) => *i < start,
        });
    }

    fn show_more_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(at) = self.menu_toggle_guard.take() {
            if at.elapsed() < std::time::Duration::from_millis(350) {
                return;
            }
        }

        if let Some(ctx) = self.active_menu.take() {
            ctx.update(cx, |m, cx| {
                m.set_on_close(None);
                m.close(window, cx);
            });
            cx.notify();
            return;
        }

        let mut menu_items = Vec::new();
        let this_weak = cx.entity().downgrade();

        if self.on_attach.is_none() {
            let this_weak_clone = this_weak.clone();
            menu_items.push(
                PopupMenuItem::item(
                    "detach",
                    i18n!(cx, "dock.panel.detach"),
                    move |_window, cx| {
                        if let Some(this) = this_weak_clone.upgrade() {
                            let _ = this.update(cx, |this, cx| {
                                this.detach_panel(cx);
                            });
                        }
                    },
                )
                .icon(AppIcon::Detach),
            );
        }

        if let Some(on_attach) = &self.on_attach {
            let on_attach = on_attach.clone();
            let active_tab_id = if self.tabs.is_empty() {
                String::new()
            } else {
                self.tabs[self.active_tab_index.unwrap_or(0)]
                    .metadata(cx)
                    .id
                    .0
                    .clone()
            };
            menu_items.push(
                PopupMenuItem::item(
                    "attach",
                    i18n!(cx, "dock.panel.attach"),
                    move |window, cx| {
                        on_attach(&active_tab_id, window, cx);
                        window.remove_window();
                    },
                )
                .icon(AppIcon::Attach),
            );
        }

        let this_weak_close = this_weak.clone();
        let bounds = self.more_button_bounds;
        let more_reg = self.overlay_registry.clone();
        let menu = cx.new(move |cx| {
            PopupMenu::new(cx, menu_items, bounds.origin, more_reg, None)
                .trigger_bounds(bounds)
                .direction(PopupMenuDirection::Below)
                .min_width(px(120.0))
        });

        let menu_id = menu.entity_id();
        let on_close: Arc<dyn Fn(&mut Window, &mut App) + Send + Sync> = Arc::new(move |_, cx| {
            if let Some(this) = this_weak_close.upgrade() {
                let _ = this.update(cx, |this, cx| {
                    if this.active_menu.as_ref().map(|m| m.entity_id()) == Some(menu_id) {
                        this.active_menu = None;
                        this.menu_toggle_guard = Some(std::time::Instant::now());
                        cx.notify();
                    }
                });
            }
        });
        menu.update(cx, |m, _| m.set_on_close(Some(on_close)));

        let focus_handle = menu.read(cx).focus_handle.clone();
        window.focus(&focus_handle, cx);
        self.active_menu = Some(menu);
        cx.notify();
    }
}

impl Focusable for DockPanel {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.active_tab()
            .and_then(|tab| tab.focus_handle(cx))
            .unwrap_or_else(|| self.focus_handle.clone())
    }
}

/// Linearly interpolate between two packed `0xRRGGBBAA` colors (also handles
/// the alpha byte). Used to smoothly fade toolbar button accents between their
/// neutral and active states. `t` is clamped to `[0, 1]`.
fn lerp_rgba(a: u32, b: u32, t: f32) -> u32 {
    let t = t.clamp(0.0, 1.0);
    let ar = ((a >> 16) & 0xff) as f32;
    let ag = ((a >> 8) & 0xff) as f32;
    let ab = (a & 0xff) as f32;
    let aa = ((a >> 24) & 0xff) as f32;
    let br = ((b >> 16) & 0xff) as f32;
    let bg = ((b >> 8) & 0xff) as f32;
    let bb = (b & 0xff) as f32;
    let ba = ((b >> 24) & 0xff) as f32;
    let r = (ar + (br - ar) * t).round() as u32;
    let g = (ag + (bg - ag) * t).round() as u32;
    let bl = (ab + (bb - ab) * t).round() as u32;
    let al = (aa + (ba - aa) * t).round() as u32;
    (al << 24) | (r << 16) | (g << 8) | bl
}

impl Render for DockPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_context(cx);

        // 1. Resize Dragging global canvas overlay
        let drag_entity = cx.entity().downgrade();
        let drag_overlay = if let Some(drag) = self.resize_dragging {
            let cursor = match drag.edge {
                DockResizeEdge::Top | DockResizeEdge::Bottom => CursorStyle::ResizeUpDown,
                DockResizeEdge::Left | DockResizeEdge::Right => CursorStyle::ResizeLeftRight,
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
                        window.on_mouse_event(move |e: &MouseMoveEvent, phase, _window, cx| {
                            if phase != DispatchPhase::Bubble {
                                return;
                            }
                            if let Some(entity) = ent.upgrade() {
                                let _ = entity.update(cx, |this, cx| {
                                    if let Some(drag) = this.resize_dragging {
                                        let delta_x =
                                            f32::from(e.position.x - drag.start_mouse_pos.x);
                                        let delta_y =
                                            f32::from(e.position.y - drag.start_mouse_pos.y);
                                        let min_center = 220.0;
                                        let min_size = this.required_min_size(cx);
                                        let win_w = f32::from(_window.viewport_size().width);
                                        let window_limit = (win_w - min_center).max(0.0);
                                        let max_by_window = window_limit.min(800.0);
                                        let effective_min = min_size.min(max_by_window);
                                        let collapse_threshold = (effective_min - 25.0).max(60.0);
                                        match drag.edge {
                                            DockResizeEdge::Left => {
                                                let raw_size = drag.start_size - delta_x;
                                                if raw_size < collapse_threshold {
                                                    this.resize_dragging = None;
                                                    cx.emit(DockPanelHideEvent);
                                                    cx.emit(DockPanelEvent);
                                                    cx.notify();
                                                    return;
                                                }
                                                this.size = raw_size.clamp(effective_min, max_by_window);
                                            }
                                            DockResizeEdge::Right => {
                                                let raw_size = drag.start_size + delta_x;
                                                if raw_size < collapse_threshold {
                                                    this.resize_dragging = None;
                                                    cx.emit(DockPanelHideEvent);
                                                    cx.emit(DockPanelEvent);
                                                    cx.notify();
                                                    return;
                                                }
                                                this.size = raw_size.clamp(effective_min, max_by_window);
                                            }
                                            DockResizeEdge::Top => {
                                                let raw_size = drag.start_size - delta_y;
                                                let bottom_collapse = 80.0;
                                                if raw_size < bottom_collapse {
                                                    this.resize_dragging = None;
                                                    cx.emit(DockPanelHideEvent);
                                                    cx.emit(DockPanelEvent);
                                                    cx.notify();
                                                    return;
                                                }
                                                this.size = raw_size.clamp(120.0, 800.0);
                                            }
                                            DockResizeEdge::Bottom => {
                                                let raw_size = drag.start_size + delta_y;
                                                let top_collapse = 80.0;
                                                if raw_size < top_collapse {
                                                    this.resize_dragging = None;
                                                    cx.emit(DockPanelHideEvent);
                                                    cx.emit(DockPanelEvent);
                                                    cx.notify();
                                                    return;
                                                }
                                                this.size = raw_size.clamp(120.0, 800.0);
                                            }
                                        }
                                        cx.emit(DockPanelEvent);
                                        cx.notify();
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
                                    if this.resize_dragging.is_some() {
                                        this.resize_dragging = None;
                                        cx.notify();
                                    }
                                });
                            }
                        });
                    },
                )
                .absolute()
                .inset_0(),
            )
        } else {
            None
        };

        // If completely hidden
        if self.collapse_state == PanelCollapseState::Hidden {
            return div().id(self.id.clone());
        }

        // Render resize handle on the configured edge (if normal mode)
        let handle_el = if self.mode == PanelMode::Normal {
            if let Some(edge) = self.resize_edge {
                let is_horizontal = match edge {
                    DockResizeEdge::Top | DockResizeEdge::Bottom => true,
                    DockResizeEdge::Left | DockResizeEdge::Right => false,
                };
                let drag_entity_for_handle = cx.entity().downgrade();
                let is_dragging = self.resize_dragging.is_some();
                Some(
                    ResizeHandle::new(
                        is_horizontal,
                        t.border,
                        t.border_active,
                        move |pos, app_cx| {
                            if let Some(entity) = drag_entity_for_handle.upgrade() {
                                let _ = entity.update(app_cx, |this, cx| {
                                    let start_size = this.size;
                                    this.resize_dragging = Some(ResizeDragState {
                                        edge,
                                        start_mouse_pos: pos,
                                        start_size,
                                    });
                                    cx.notify();
                                });
                            }
                        },
                    )
                    .with_size(f32::from(ui_space_card_gap(cx)))
                    .is_active(is_dragging),
                )
            } else {
                None
            }
        } else {
            None
        };

        // 1. Render Action Buttons
        let scale = crate::tokens::ui_scale_factor(cx);
        let action_ap = ControlAppearance::resolve(
            ControlSize::Compact,
            ControlVariant::Ghost,
            &p,
            crate::tokens::get_ui_density(cx),
            crate::tokens::ui_text_scale(cx),
        );
        let action_pad_x = px((f32::from(action_ap.height) - f32::from(action_ap.icon_size)) / 2.0);
        let mut action_elements: Vec<AnyElement> = Vec::new();
        let mut actions_w = 0.0;
        let action_btn_w = f32::from(action_ap.height);

        if let Some(active_panel) = self.active_tab() {
            for (idx, action) in active_panel.custom_actions(cx).into_iter().enumerate() {
                let callback = action.callback.clone();
                let tooltip = action.tooltip.clone();
                actions_w += action_btn_w;
                action_elements.push(
                    Button::new(format!("custom-action-{}", idx), &t)
                        .text()
                        .small()
                        .px(action_pad_x)
                        .icon_left(action.icon)
                        .tooltip(tooltip)
                        .on_click(move |_, window, cx| {
                            callback(window, cx);
                        })
                        .into_any_element(),
                );
            }
        }

        let weak_more = cx.entity().downgrade();
        let more_tooltip = i18n!(cx, "common.action.more");
        let more_menu_open = self.active_menu.is_some();
        actions_w += action_btn_w;
        action_elements.push(
            div()
                .id("more-action-btn")
                .group("more-action-btn")
                .cursor_pointer()
                .h(action_ap.height)
                .px(action_pad_x)
                .flex()
                .items_center()
                .justify_center()
                .rounded(action_ap.radius)
                .hover(move |s| s.bg(p.surface_hover))
                .child(
                    AppIcon::MoreMenu
                        .size(ui_icon_std_ts(cx))
                        .text_color(p.text_secondary)
                        .group_hover("more-action-btn", |s| s.text_color(p.text_primary)),
                )
                .when(!more_menu_open, |this| {
                    this.tooltip(move |_, cx| {
                        cx.new(|_| crate::tooltip::Tooltip::new(more_tooltip.clone()))
                            .into()
                    })
                })
                .child(
                    canvas(
                        move |bounds, _window, cx| {
                            if let Some(this) = weak_more.upgrade() {
                                let _ = this.update(cx, |this, _| {
                                    this.more_button_bounds = bounds;
                                });
                            }
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .inset_0(),
                )
                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                    cx.stop_propagation();
                })
                .on_click(cx.listener(|this, _, window, cx| {
                    this.show_more_menu(window, cx);
                }))
                .into_any_element(),
        );

        if self.fill_window && !self.suppress_reattach {
            if let Some(on_attach) = &self.on_attach {
                let on_attach_btn = on_attach.clone();
                let active_tab_id_btn = if self.tabs.is_empty() {
                    String::new()
                } else {
                    self.tabs[self.active_tab_index.unwrap_or(0)]
                        .metadata(cx)
                        .id
                        .0
                        .clone()
                };
                let reattach_tooltip = i18n!(cx, "dock.panel.attach");
                actions_w += action_btn_w;
                action_elements.push(
                    div()
                        .id("reattach-header-btn-wrapper")
                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                            cx.stop_propagation();
                        })
                        .child(
                            div()
                                .id("reattach-header-btn")
                                .group("reattach-header-btn")
                                .cursor_pointer()
                                .h(action_ap.height)
                                .px(action_pad_x)
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(action_ap.radius)
                                .hover(move |s| s.bg(p.surface_hover))
                                .child(
                                    AppIcon::Attach
                                        .size(action_ap.icon_size)
                                        .text_color(p.text_secondary)
                                        .group_hover("reattach-header-btn", |s| s.text_color(p.text_primary)),
                                )
                                .on_click(move |_, window, cx| {
                                    on_attach_btn(&active_tab_id_btn, window, cx);
                                    window.remove_window();
                                })
                                .tooltip(move |_, cx| {
                                    cx.new(|_| crate::tooltip::Tooltip::new(reattach_tooltip.clone()))
                                        .into()
                                }),
                        )
                        .into_any_element(),
                );
            }
        }

        actions_w += 9.0;
        action_elements.push(
            div()
                .id("action-divider")
                .h(SPACE_XL)
                .w(px(1.0))
                .my(px(2.0))
                .bg(p.border_subtle)
                .into_any_element(),
        );

        if !self.fill_window {
            let weak_expand = cx.entity().downgrade();
            let is_maximized = self.mode == PanelMode::Maximized;
            let expand_icon = if is_maximized {
                AppIcon::FullscreenExit
            } else {
                AppIcon::Fullscreen
            };
            let expand_tooltip = if is_maximized {
                i18n!(cx, "dock.action.exit_fullscreen")
            } else {
                i18n!(cx, "dock.action.expand")
            };
            actions_w += action_btn_w;
            action_elements.push(
                div()
                    .id("expand-action-btn-wrapper")
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .child(
                        div()
                            .id("expand-action-btn")
                            .group("expand-action-btn")
                            .cursor_pointer()
                            .h(action_ap.height)
                            .px(action_pad_x)
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(action_ap.radius)
                            .hover(move |s| s.bg(p.surface_hover))
                            .child(
                                expand_icon
                                    .size(action_ap.icon_size)
                                    .text_color(p.text_secondary)
                                    .group_hover("expand-action-btn", |s| s.text_color(p.text_primary)),
                            )
                            .on_click(move |_, _window, cx| {
                                if let Some(this) = weak_expand.upgrade() {
                                    let _ = this.update(cx, |this, cx| {
                                        this.mode = if this.mode == PanelMode::Maximized {
                                            PanelMode::Normal
                                        } else {
                                            PanelMode::Maximized
                                        };
                                        cx.emit(DockPanelEvent);
                                        cx.notify();
                                    });
                                }
                            })
                            .tooltip(move |_, cx| {
                                cx.new(|_| crate::tooltip::Tooltip::new(expand_tooltip.clone()))
                                    .into()
                            }),
                    )
                    .into_any_element(),
            );
        }

        if self.mode != PanelMode::Maximized && !self.fill_window {
            let weak_close = cx.entity().downgrade();
            let close_tooltip = i18n!(cx, "dock.action.hide");
            actions_w += action_btn_w;
            action_elements.push(
                div()
                    .id("close-action-btn-wrapper")
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .child(
                        div()
                            .id("close-action-btn")
                            .group("close-action-btn")
                            .cursor_pointer()
                            .h(action_ap.height)
                            .px(action_pad_x)
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(action_ap.radius)
                            .hover(move |s| s.bg(p.surface_hover))
                            .child(
                                AppIcon::Minimize
                                    .size(action_ap.icon_size)
                                    .text_color(p.text_secondary)
                                    .group_hover("close-action-btn", |s| s.text_color(p.text_primary)),
                            )
                            .on_click(move |_, _window, cx| {
                                if let Some(this) = weak_close.upgrade() {
                                    let _ = this.update(cx, |_, cx| {
                                        cx.emit(DockPanelHideEvent);
                                        cx.emit(DockPanelEvent);
                                        cx.notify();
                                    });
                                }
                            })
                            .tooltip(move |_, cx| {
                                cx.new(|_| crate::tooltip::Tooltip::new(close_tooltip.clone()))
                                    .into()
                            }),
                    )
                    .into_any_element(),
            );
        }

        let mut left_window_controls: Option<AnyElement> = None;

        if self.fill_window {
            if let Some(on_attach) = &self.on_attach {
                let is_custom = crate::decorations::is_custom_titlebar(window, cx);
                if is_custom && crate::overlay::detached_needs_controls(window) {
                    let on_attach_close = on_attach.clone();
                    let active_tab_id_close = if self.tabs.is_empty() {
                        String::new()
                    } else {
                        self.tabs[self.active_tab_index.unwrap_or(0)]
                            .metadata(cx)
                            .id
                            .0
                            .clone()
                    };
                    let decoration_config = crate::decorations::get_window_decoration_config(cx);
                    let control_icon_sz = crate::decorations::get_window_control_icon_size(cx);

                    let close_fn = std::sync::Arc::new(move |window: &mut Window, cx: &mut App| {
                        on_attach_close(&active_tab_id_close, window, cx);
                        window.remove_window();
                    });

                    let controls_bar = crate::title_bar::render_window_controls(
                        "detached-dock-ctrl",
                        window,
                        &decoration_config,
                        Some(control_icon_sz),
                        Some(close_fn),
                        &t,
                        cx,
                    );

                    if decoration_config.position == crate::decorations::WindowButtonPosition::Left {
                        left_window_controls = Some(controls_bar.into_any_element());
                    } else {
                        actions_w += 80.0 * scale;
                        action_elements.push(controls_bar.into_any_element());
                    }
                }
            }
        }

        let toolbar_items: Vec<ToolbarItem> = self
            .active_tab()
            .map(|p| p.toolbar_elements(cx))
            .unwrap_or_default();
        let mut toolbar_elts = Vec::new();
        let mut toolbar_w = 0.0;
        let mut toolbar_items_count: usize = 0;

        for (tb_idx, item) in toolbar_items.into_iter().enumerate() {
            let item_w = item.estimated_width();
            let el: AnyElement = match item {
                ToolbarItem::IconButton {
                    icon,
                    tooltip,
                    enabled,
                    on_click,
                    accent,
                    active,
                } => {
                    let ico = icon;
                    let en = enabled;
                    let ttip = tooltip.clone();
                    let cb = on_click.clone();

                    let accent_color: Option<u32> = accent.map(|a| match a {
                        AccentColor::Success => t.success,
                        AccentColor::Warning => t.warning,
                        AccentColor::Error => t.error,
                    });

                    let neutral: u32 = if en { t.text_secondary } else { t.text_muted };
                    let group_id = SharedString::from(format!("toolbar-btn-{tb_idx}"));
                    let icon_el: AnyElement = if let Some(ac) = accent_color {
                        let _tint = (ac << 8) | 0x26u32;
                        let _tint_hover = (ac << 8) | 0x40u32;
                        let anim_id = format!("tb-icon-{tb_idx}-{}", active as u8);
                        let from = if active { neutral } else { ac };
                        let to = if active { ac } else { neutral };
                        ico.size(ui_icon_std_ts(cx))
                            .text_color(rgb(neutral))
                            .group_hover(group_id.clone(), |s| s.text_color(rgb(ac)))
                            .with_animation(
                                anim_id,
                                Animation::new(Duration::from_millis(90))
                                    .with_easing(|d| 1.0 - (1.0 - d).powi(3)),
                                move |this, delta| this.text_color(rgb(lerp_rgba(from, to, delta))),
                            )
                            .into_any_element()
                    } else {
                        ico.size(ui_icon_std_ts(cx))
                            .text_color(rgb(neutral))
                            .group_hover(group_id.clone(), |s| s.text_color(p.text_primary))
                            .into_any_element()
                    };

                    let btn = div()
                        .id(group_id.clone())
                        .group(group_id)
                        .cursor_pointer()
                        .p(SPACE_XS)
                        .rounded(RADIUS_STD)
                        .relative();

                    let btn = if let Some(ac) = accent_color {
                        let tint = (ac << 8) | 0x26u32;
                        let tint_hover = (ac << 8) | 0x40u32;
                        let bg_anim_id = format!("tb-bg-{tb_idx}-{}", active as u8);
                        let bg_from = if active { 0x00000000u32 } else { tint };
                        let bg_to = if active { tint } else { 0x00000000u32 };
                        btn.stateful_behavior(HoverBehavior {
                            hover_bg: rgba(tint_hover).into(),
                            ..Default::default()
                        })
                        .child(
                            div()
                                .absolute()
                                .inset_0()
                                .rounded(RADIUS_STD)
                                .with_animation(
                                    bg_anim_id,
                                    Animation::new(Duration::from_millis(90))
                                        .with_easing(|d| 1.0 - (1.0 - d).powi(3)),
                                    move |this, delta| {
                                        this.bg(rgba(lerp_rgba(bg_from, bg_to, delta)))
                                    },
                                ),
                        )
                        .child(icon_el)
                    } else {
                        btn.hover(move |s| s.bg(p.surface_hover))
                            .child(icon_el)
                    };

                    let btn = btn.tooltip(move |_, cx| {
                        cx.new(|_| crate::tooltip::Tooltip::new(ttip.clone()))
                            .into()
                    });

                    let btn = if en {
                        btn.on_click(move |_, window, cx| {
                            cb(window, cx);
                        })
                    } else {
                        btn
                    };

                    btn.into_any_element()
                }
                ToolbarItem::Separator => {
                    div()
                        .flex_shrink_0()
                        .w(px(1.0))
                        .h(SPACE_XL)
                        .bg(p.border_subtle)
                        .into_any_element()
                }
                ToolbarItem::Label(lbl) => {
                    div()
                        .flex_shrink_0()
                        .text_size(ui_text_md(cx))
                        .text_color(p.text_secondary)
                        .child(lbl.clone())
                        .into_any_element()
                }
                ToolbarItem::TextInput {
                    entity,
                    width_px,
                    suffix,
                    on_enter,
                } => {
                    let enter_cb = on_enter.clone();
                    let input_el = Input::new(&entity)
                        .w(px(width_px))
                        .size(ControlSize::Compact)
                        .text_size(ui_text_md(cx));
                    let input_box = match enter_cb {
                        Some(cb) => div()
                            .flex_shrink_0()
                            .on_key_down(move |event: &KeyDownEvent, window, cx| {
                                if event.keystroke.key.as_str() == "enter" {
                                    cb(window, cx);
                                }
                            })
                            .child(input_el)
                            .into_any_element(),
                        None => input_el.into_any_element(),
                    };
                    match suffix {
                        Some(unit) => div()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .child(input_box)
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .pl(px(2.0))
                                    .text_size(ui_text_md(cx))
                                    .text_color(p.text_muted)
                                    .child(unit.clone()),
                            )
                            .into_any_element(),
                        None => input_box,
                    }
                }
                ToolbarItem::Custom(el) => {
                    el
                }
            };
            toolbar_w += item_w;
            toolbar_items_count += 1;
            toolbar_elts.push(el);
        }
        if toolbar_items_count > 0 {
            toolbar_w += toolbar_items_count.saturating_sub(1) as f32 * 6.0 + 12.0;
        }

        let has_toolbar = !toolbar_elts.is_empty();

        // 3. Tab Overflow Calculation & Tab Elements Rendering
        let scale = crate::tokens::ui_scale_factor(cx);
        let is_detached_window = self.fill_window && self.on_attach.is_some();
        let is_custom_titlebar = crate::decorations::is_custom_titlebar(window, cx);
        let decoration_config = crate::decorations::get_window_decoration_config(cx);
        let custom_margin_px = decoration_config.custom_margin.unwrap_or(8.0) * scale;
        let needs_controls = crate::overlay::detached_needs_controls(window);
        let is_left_controls = decoration_config.position == crate::decorations::WindowButtonPosition::Left;
        let is_right_controls = decoration_config.position == crate::decorations::WindowButtonPosition::Right;

        let left_header_padding = if is_detached_window && is_custom_titlebar {
            if is_left_controls {
                custom_margin_px
            } else if cfg!(target_os = "macos") && !needs_controls {
                80.0 * scale
            } else {
                0.0
            }
        } else {
            0.0
        };

        let right_header_padding = if is_detached_window && is_custom_titlebar && is_right_controls && needs_controls {
            custom_margin_px
        } else {
            f32::from(crate::tokens::ui_space_xs(cx))
        };

        let is_vert_dock = matches!(self.resize_edge, Some(DockResizeEdge::Left) | Some(DockResizeEdge::Right));
        let total_w = if let Some(w) = self.header_measured_width {
            w
        } else if is_vert_dock {
            self.size
        } else {
            f32::from(window.viewport_size().width)
        };
        let actions_total_w = {
            let action_items_count = action_elements.len();
            let action_gaps_w = action_items_count.saturating_sub(1) as f32 * f32::from(crate::tokens::ui_space_xs(cx));
            actions_w + action_gaps_w
        };
        let effective_toolbar_w = if has_toolbar {
            toolbar_w
        } else {
            0.0
        };
        let title_overhead = f32::from(crate::tokens::ui_space_md(cx)) + f32::from(crate::tokens::ui_space_md(cx));
        let avail_for_tabs = (total_w - left_header_padding - right_header_padding - actions_total_w - effective_toolbar_w - title_overhead).max(0.0);

        let active_idx = self.active_tab_index.unwrap_or(0);
        let tab_widths: Vec<f32> = self
            .tabs
            .iter()
            .map(|panel| {
                let meta = panel.metadata(cx);
                let title = self
                    .title_overrides
                    .get(&meta.id)
                    .cloned()
                    .unwrap_or(meta.title);
                let title_w = (title.chars().count() as f32 * 7.5).max(20.0);
                let icon_w = if self.show_icons { 24.0 } else { 0.0 };
                let close_w = if meta.closable { 20.0 } else { 0.0 };
                let badge_w = if meta.badge.is_some() { 24.0 } else { 0.0 };
                f32::from(crate::tab::tab_h_padding(cx)) * 2.0 + icon_w + title_w + close_w + badge_w
            })
            .collect();

        let total_tabs_needed: f32 = tab_widths.iter().sum();

        let min_tab_w = 32.0 * scale;
        let more_btn_w = 40.0;
        let (visible_indices, overflow_indices): (Vec<usize>, Vec<usize>) =
            if self.tabs.is_empty() || total_tabs_needed <= avail_for_tabs {
                ((0..self.tabs.len()).collect(), Vec::new())
            } else if avail_for_tabs >= min_tab_w + more_btn_w {
                // Room is available for at least the active tab + the More button
                let budget = avail_for_tabs - more_btn_w;
                let mut vis = vec![active_idx];
                let mut over = Vec::new();

                let active_tab_ideal_w = tab_widths.get(active_idx).copied().unwrap_or(min_tab_w);
                let mut remaining_budget = (budget - active_tab_ideal_w).max(0.0);

                for (i, &w) in tab_widths.iter().enumerate() {
                    if i == active_idx {
                        continue;
                    }
                    if remaining_budget >= w {
                        vis.push(i);
                        remaining_budget -= w;
                    } else {
                        over.push(i);
                    }
                }
                vis.sort_unstable();
                (vis, over)
            } else {
                // Room is less than (min_tab_w + more_btn_w); fold all tabs into overflow
                (Vec::new(), (0..self.tabs.len()).collect())
            };
        let mut tab_elements = Vec::new();
        for &i in &visible_indices {
            if let Some(panel) = self.tabs.get(i) {
                let meta = panel.metadata(cx);
                let active = self.active_tab_index == Some(i);
                let is_renaming = self
                    .rename_state
                    .as_ref()
                    .is_some_and(|s| s.target == meta.id);
                let title = self
                    .title_overrides
                    .get(&meta.id)
                    .cloned()
                    .unwrap_or(meta.title);

                let tab_hover_progress = self
                    .hover_progress
                    .get(&HoverChannel::Tab(i))
                    .copied()
                    .unwrap_or(0.0);

                let text_color = if active {
                    p.text_primary
                } else {
                    let base = p.text_secondary;
                    let selected = p.text_primary;
                    base.blend(selected.alpha(tab_hover_progress))
                };

                let this_weak = cx.entity().downgrade();
                let tab_id = format!("tab-{}", meta.id.0);

                let tab_content = if is_renaming {
                    let input_state = self.rename_state.as_ref().unwrap().input.clone();
                    div()
                        .w(px(100.0))
                        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                            if event.keystroke.key == "Enter" {
                                this.commit_rename(window, cx);
                                cx.stop_propagation();
                            } else if event.keystroke.key == "Escape" {
                                cancel_rename(&mut this.rename_state);
                                cx.notify();
                                cx.stop_propagation();
                            }
                        }))
                        .child(Input::new(&input_state).text_size(ui_text_md(cx)))
                } else {
                    h_flex()
                        .items_center()
                        .gap(SPACE_SM)
                        .overflow_hidden()
                        .when(self.show_icons, |this| {
                            this.child(
                                meta.icon
                                    .size(ui_icon_std_ts(cx))
                                    .flex_shrink_0()
                                    .text_color(text_color),
                            )
                        })
                        .child(
                            div()
                                .truncate()
                                .text_size(ui_text_md(cx))
                                .text_color(text_color)
                                .child(title),
                        )
                };

                let badge_el = if let Some(badge) = meta.badge {
                    match badge {
                        PanelBadge::Count(c) => Some(
                            div()
                                .px(SPACE_XS)
                                .bg(p.surface_hover)
                                .rounded(RADIUS_LG)
                                .text_size(ui_text_sm(cx))
                                .text_color(p.text_muted)
                                .child(c.to_string()),
                        ),
                        PanelBadge::Text(txt) => Some(
                            div()
                                .px(SPACE_XS)
                                .bg(p.surface_hover)
                                .rounded(RADIUS_STD)
                                .text_size(ui_text_sm(cx))
                                .text_color(p.text_muted)
                                .child(txt),
                        ),
                        PanelBadge::StatusDot(color) => {
                            Some(div().size(px(8.0)).bg(color).rounded(RADIUS_STD))
                        }
                    }
                } else {
                    None
                };

                let close_tooltip = format!("{} (Ctrl+W)", i18n!(cx, "dock.tab.close"));
                let close_weak = this_weak.clone();
                let close_size = ui_text_md(cx) + px(4.0);
                let close_btn = if meta.closable {
                    Some(
                        div()
                            .id(format!("close-btn-{}", i))
                            .cursor_pointer()
                            .flex_shrink_0()
                            .w(close_size)
                            .h(close_size)
                            .rounded(RADIUS_MD)
                            .flex()
                            .items_center()
                            .justify_center()
                            .opacity(tab_hover_progress)
                            .hover(|s| s.bg(rgba(0xf14c4c99)))
                            .when(tab_hover_progress > 0.01, |d| {
                                d.on_mouse_down(MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation();
                                })
                                .on_click(move |_, window, cx| {
                                    cx.stop_propagation();
                                    if let Some(this) = close_weak.upgrade() {
                                        let _ = this.update(cx, |this, cx| {
                                            this.close_tab(i, window, cx);
                                        });
                                    }
                                })
                            })
                            .child(AppIcon::Close.size(ui_text_md(cx)).text_color(p.text_muted))
                            .tooltip(move |_, cx| {
                                cx.new(|_| crate::tooltip::Tooltip::new(close_tooltip.clone()))
                                    .into()
                            }),
                    )
                } else {
                    None
                };

                let select_weak = this_weak.clone();
                let menu_weak = this_weak.clone();

                let tab_el = tab_style(
                    div().id(tab_id),
                    &t,
                    active,
                    cx,
                )
                .on_hover(move |&hovered, window, cx| {
                    if let Some(this) = this_weak.clone().upgrade() {
                        let _ = this.update(cx, |this, cx| {
                            this.set_hover(HoverChannel::Tab(i), hovered, window, cx);
                        });
                    }
                })
                .gap(SPACE_XS)
                .on_mouse_down(MouseButton::Left, move |_e, window, cx| {
                    cx.stop_propagation();
                    if let Some(this) = select_weak.upgrade() {
                        let _ = this.update(cx, |this, cx| {
                            this.select_tab(i, cx);
                            this.focus_active_tab(window, cx);
                        });
                    }
                })
                .on_mouse_down(MouseButton::Right, move |e, window, cx| {
                    cx.stop_propagation();
                    if let Some(this) = menu_weak.upgrade() {
                        let _ = this.update(cx, |this, cx| {
                            this.show_context_menu(Some(i), e.position, window, cx);
                        });
                    }
                })
                .child(tab_content)
                .when_some(badge_el, |this, b| this.child(b))
                .when_some(close_btn, |this, c| this.child(c));

                tab_elements.push(tab_el.into_any_element());
            }
        }

        let more_btn_element: Option<AnyElement> = if !overflow_indices.is_empty() {
            let overflow_weak = cx.entity().downgrade();
            let overflow_count = overflow_indices.len();
            let has_active_in_overflow = overflow_indices.contains(&active_idx);
            let more_tabs_tooltip = i18n!(cx, "dock.tab.more_tabs");
            let more_tabs_id = format!("more-tabs-btn-{}", self.id);

            let more_btn = h_flex()
                .id(more_tabs_id)
                .cursor_pointer()
                .h(px(tab_height(cx) - 6.0))
                .flex_shrink_0()
                .items_center()
                .justify_center()
                .px(SPACE_SM)
                .rounded(RADIUS_MD)
                .gap(SPACE_2XS)
                .hover(|s| s.bg(p.surface_hover))
                .child(
                    AppIcon::MoreMenu
                        .size(ui_icon_std_ts(cx))
                        .text_color(if has_active_in_overflow {
                            p.border_active
                        } else {
                            p.text_secondary
                        }),
                )
                .child(
                    div()
                        .px(SPACE_XS)
                        .bg(p.surface_hover)
                        .rounded(RADIUS_LG)
                        .text_size(ui_text_sm(cx))
                        .text_color(p.text_muted)
                        .child(overflow_count.to_string()),
                )
                .tooltip(move |_, cx| {
                    cx.new(|_| crate::tooltip::Tooltip::new(more_tabs_tooltip.clone())).into()
                })
                .on_mouse_down(MouseButton::Left, move |e, window, cx| {
                    cx.stop_propagation();
                    if let Some(this) = overflow_weak.upgrade() {
                        let _ = this.update(cx, |this, cx| {
                            this.show_overflow_tabs_menu(overflow_indices.clone(), e.position, window, cx);
                        });
                    }
                });

            Some(more_btn.into_any_element())
        } else {
            None
        };

        // 4. Render Panel Body Content (based on status or active tab)
        let body_content = if self.collapse_state == PanelCollapseState::Collapsed {
            div().into_any_element()
        } else if self.tabs.is_empty() {
            v_flex()
                .w_full()
                .h_full()
                .items_center()
                .justify_center()
                .child(crate::empty_state::empty_state(
                    i18n!(cx, "dock.state.empty"),
                    &t,
                    cx,
                ))
                .into_any_element()
        } else if let Some(active_panel) = self.active_tab() {
            let meta = active_panel.metadata(cx);
            if meta.loading {
                v_flex()
                    .w_full()
                    .h_full()
                    .items_center()
                    .justify_center()
                    .gap(SPACE_MD)
                    .child(
                        div()
                            .text_size(ui_text_md(cx))
                            .text_color(p.text_muted)
                            .child(i18n!(cx, "dock.state.loading")),
                    )
                    .into_any_element()
            } else if let Some(ref err) = meta.error {
                let err_msg = err.clone();
                let retry_weak = cx.entity().downgrade();
                let idx = self.active_tab_index.unwrap_or(0);
                v_flex()
                    .w_full()
                    .h_full()
                    .items_center()
                    .justify_center()
                    .gap(SPACE_MD)
                    .child(
                        div()
                            .text_size(ui_text_md(cx))
                            .text_color(rgb(t.error))
                            .child(format!("{}: {}", i18n!(cx, "dock.state.error"), err_msg)),
                    )
                    .child(
                        button_primary("retry-btn", i18n!(cx, "common.action.retry"), &t).on_click(
                            move |_, _window, cx| {
                                if let Some(this) = retry_weak.upgrade() {
                                    let _ = this.update(cx, |this, cx| {
                                        this.tabs[idx]
                                            .trigger_lifecycle(PanelLifecycleEvent::Open, cx);
                                    });
                                }
                            },
                        ),
                    )
                    .into_any_element()
            } else {
                active_panel.view.clone().into_any_element()
            }
        } else {
            div().into_any_element()
        };

        // Render StatusBar (optional)
        let status_text = self.active_tab().and_then(|p| p.status_text(cx));
        let status_bar_el = if let Some(status) = status_text {
            Some(
                h_flex()
                    .w_full()
                    .px(SPACE_LG)
                    .py(SPACE_XS)
                    .border_t_1()
                    .border_color(p.border_subtle)
                    .text_size(ui_text_ms(cx))
                    .text_color(p.text_muted)
                    .child(status),
            )
        } else {
            None
        };

        let is_vertical_edge = matches!(
            self.resize_edge,
            Some(DockResizeEdge::Left) | Some(DockResizeEdge::Right)
        );

        let is_windowed = !window.is_maximized() && !window.is_fullscreen();
        let is_custom_titlebar = crate::decorations::is_custom_titlebar(window, cx);
        let corner_r = px(self.window_corner_radius.unwrap_or_else(|| {
            if is_custom_titlebar && is_windowed {
                crate::decorations::get_window_corner_radius(cx)
            } else {
                0.0
            }
        }));

        let is_maximized_or_fullscreen =
            self.mode == PanelMode::Maximized || self.mode == PanelMode::Fullscreen;

        // Layout sizes depending on edge and collapse state
        let root_style = div()
            .id(self.id.clone())
            .track_focus(&self.focus_handle)
            .relative()
            .on_key_down(cx.listener(|this, event, window, cx| {
                this.handle_key_down(event, window, cx);
            }))
            .flex()
            .when(is_windowed && is_maximized_or_fullscreen, |d| {
                d.rounded_bl(corner_r).rounded_br(corner_r)
            })
            .when(is_windowed && self.fill_window, |d| d.rounded(corner_r))
            .when(!is_maximized_or_fullscreen && !self.fill_window, |d| {
                d.rounded(RADIUS_CARD)
            })
            .when(is_vertical_edge, |d| d.flex_row())
            .bg(if !is_maximized_or_fullscreen && !self.fill_window {
                p.surface_card
            } else {
                surface_bg(t.bg_panel, cx)
            })
            .border_1()
            .border_color(p.border_subtle)
            .when_some(drag_overlay, |this, overlay| this.child(overlay));

        // Set dimensions based on collapse state and size parameter
        let sized_root = if self.fill_window {
            root_style.size_full()
        } else {
            match self.collapse_state {
                PanelCollapseState::Hidden => root_style,
                PanelCollapseState::Collapsed => {
                    let collapsed_dim = px(tab_height(cx));
                    if is_vertical_edge {
                        root_style.w(collapsed_dim).h_full()
                    } else {
                        root_style.h(collapsed_dim).w_full()
                    }
                }
                PanelCollapseState::Normal => root_style.size_full(),
            }
        };

        // Construct the content area (Header + Body + StatusBar)
        let content_col = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_hidden()
            .when(is_windowed && is_maximized_or_fullscreen, |d| {
                d.rounded_bl(corner_r).rounded_br(corner_r)
            })
            .when(!is_maximized_or_fullscreen && !self.fill_window, |d| {
                d.rounded(RADIUS_CARD)
            })
            // 2. Header — 右键菜单作用于整个标签栏区域，不受标签数量影响。
            // 注意：header 不设置自己的背景，直接透出 dock 根层
            // `surface_bg(t.bg_panel)` 的半透明表面，使其与内容区（body
            // 包裹层本身也无背景）同为单层透明，避免 header 在根层之上再叠加
            // 一层背景导致透明度被累积（双层叠加变得接近实心）。这样底/右 dock
            // 的 header 与内容区的透明效果保持一致。
            .child({
                let is_detached_window = self.fill_window && self.on_attach.is_some();
                let is_custom_titlebar = crate::decorations::is_custom_titlebar(window, cx);
                let header_height = if is_detached_window && is_custom_titlebar {
                    crate::decorations::get_titlebar_height(cx)
                } else {
                    tab_height(cx)
                };

                let decoration_config = crate::decorations::get_window_decoration_config(cx);
                let scale = crate::tokens::ui_scale_factor(cx);
                let custom_margin_px = decoration_config.custom_margin.unwrap_or(8.0) * scale;
                let needs_controls = crate::overlay::detached_needs_controls(window);

                let is_left_controls =
                    decoration_config.position == crate::decorations::WindowButtonPosition::Left;
                let is_right_controls =
                    decoration_config.position == crate::decorations::WindowButtonPosition::Right;

                let left_header_padding = if is_detached_window && is_custom_titlebar {
                    if is_left_controls {
                        px(custom_margin_px)
                    } else if cfg!(target_os = "macos") && !needs_controls {
                        px(80.0 * scale)
                    } else {
                        px(0.0)
                    }
                } else {
                    px(0.0)
                };

                let right_header_padding = if is_detached_window
                    && is_custom_titlebar
                    && is_right_controls
                    && needs_controls
                {
                    px(custom_margin_px)
                } else {
                    crate::tokens::ui_space_xs(cx)
                };

                let is_title_mode = match self.header_mode {
                    DockHeaderMode::TitleOnly => true,
                    DockHeaderMode::Tabs => false,
                    DockHeaderMode::Auto => self.tabs.len() <= 1,
                };

                let center_header_content = if is_title_mode {
                    let active_panel = self.active_tab();
                    let active_meta = active_panel.as_ref().map(|p| p.metadata(cx));
                    let raw_title_str = active_meta
                        .as_ref()
                        .map(|m| {
                            self.title_overrides
                                .get(&m.id)
                                .cloned()
                                .unwrap_or_else(|| m.title.clone())
                        })
                        .unwrap_or_default();
                    let icon_opt = active_meta.as_ref().map(|m| m.icon);

                    let icon_space = if self.show_icons && icon_opt.is_some() { 24.0 } else { 0.0 };
                    let avail_title_w = (avail_for_tabs - icon_space).max(0.0);
                    let display_title = Self::truncate_with_bottom_dots(&raw_title_str, avail_title_w);

                    h_flex()
                        .flex_1()
                        .h_full()
                        .min_w_0()
                        .overflow_hidden()
                        .items_center()
                        .gap(SPACE_SM)
                        .pl(SPACE_MD)
                        .when(self.show_icons, |h| {
                            if let Some(icon) = icon_opt {
                                h.child(
                                    icon.size(ui_icon_std_ts(cx))
                                        .flex_shrink_0()
                                        .text_color(p.text_secondary),
                                )
                            } else {
                                h
                            }
                        })
                        .child(
                            div()
                                .min_w_0()
                                .flex_shrink_0()
                                .text_size(ui_text_md(cx))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(p.text_primary)
                                .child(display_title),
                        )
                        .into_any_element()
                } else {
                    h_flex()
                        .flex_shrink_0()
                        .h_full()
                        .pl(SPACE_XS)
                        .gap(SPACE_XS)
                        .items_center()
                        .children(tab_elements)
                        .when_some(more_btn_element, |h, btn| h.child(btn))
                        .into_any_element()
                };

                let weak_hdr = cx.entity().downgrade();
                h_flex()
                    .w_full()
                    .h(px(header_height))
                    .relative()
                    .child(
                        canvas(
                            move |bounds, _window, cx| {
                                let w = f32::from(bounds.size.width);
                                if let Some(this) = weak_hdr.upgrade() {
                                    let _ = this.update(cx, |this, cx| {
                                        if this.header_measured_width.map_or(true, |old| (old - w).abs() > 1.0) {
                                            this.header_measured_width = Some(w);
                                            cx.notify();
                                        }
                                    });
                                }
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .inset_0(),
                    )
                    .pl(left_header_padding)
                    .pr(right_header_padding)
                    .border_b_1()
                    .border_color(p.border_subtle)
                    .when(!is_maximized_or_fullscreen && !self.fill_window, |d| {
                        d.rounded_t(RADIUS_CARD)
                    })
                    .justify_between()
                    .when(self.fill_window, |d| {
                        d.window_control_area(WindowControlArea::Drag).when(
                            cfg!(target_os = "linux"),
                            |d| {
                                d.on_mouse_down(MouseButton::Left, |_, window, _| {
                                    window.start_window_move();
                                })
                            },
                        )
                    })
                    .on_mouse_down(MouseButton::Right, {
                        let tabs_weak = cx.entity().downgrade();
                        move |e, window, cx| {
                            cx.stop_propagation();
                            if let Some(this) = tabs_weak.upgrade() {
                                let _ = this.update(cx, |this, cx| {
                                    this.show_context_menu(None, e.position, window, cx);
                                });
                            }
                        }
                    })
                    .when_some(left_window_controls, |h, controls| {
                        h.child(
                            h_flex()
                                .flex_shrink_0()
                                .pr(SPACE_XS)
                                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation();
                                })
                                .child(controls),
                        )
                    })
                    .child(center_header_content)
                    .child(
                        h_flex()
                            .flex_shrink_0()
                            .items_center()
                            .gap(SPACE_MD)
                            .when(has_toolbar, |r| {
                                r.child(
                                    h_flex()
                                        .flex_shrink_0()
                                        .items_center()
                                        .gap(crate::tokens::ui_space_xs(cx))
                                        .px(crate::tokens::ui_space_xs(cx))
                                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation();
                                        })
                                        .children(toolbar_elts),
                                )
                            })
                            .child(
                                h_flex()
                                    .flex_shrink_0()
                                    .gap(crate::tokens::ui_space_xs(cx))
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation();
                                    })
                                    .children(action_elements),
                            ),
                    )
            })
            // 3. Body
            .when(self.collapse_state == PanelCollapseState::Normal, |this| {
                this.child(div().flex_1().w_full().min_h(px(0.0)).child(body_content))
            })
            // 4. StatusBar
            .when_some(status_bar_el, |this, s| this.child(s));

        let card_gap = ui_space_card_gap(cx);
        let final_el = match self.resize_edge {
            Some(DockResizeEdge::Left) => sized_root
                .child(
                    div()
                        .size_full()
                        .overflow_hidden()
                        .when(is_windowed && is_maximized_or_fullscreen, |d| {
                            d.rounded_bl(corner_r).rounded_br(corner_r)
                        })
                        .when(is_windowed && !is_maximized_or_fullscreen, |d| {
                            d.rounded_br(corner_r)
                        })
                        .child(content_col),
                )
                .when_some(handle_el, |this, h| {
                    this.child(
                        div()
                            .absolute()
                            .left(-card_gap)
                            .top_0()
                            .bottom_0()
                            .w(card_gap)
                            .child(h),
                    )
                }),
            Some(DockResizeEdge::Right) => sized_root
                .child(
                    div()
                        .size_full()
                        .overflow_hidden()
                        .when(is_windowed && is_maximized_or_fullscreen, |d| {
                            d.rounded_bl(corner_r).rounded_br(corner_r)
                        })
                        .when(is_windowed && !is_maximized_or_fullscreen, |d| {
                            d.rounded_bl(corner_r)
                        })
                        .child(content_col),
                )
                .when_some(handle_el, |this, h| {
                    this.child(
                        div()
                            .absolute()
                            .right(-card_gap)
                            .top_0()
                            .bottom_0()
                            .w(card_gap)
                            .child(h),
                    )
                }),
            Some(DockResizeEdge::Top) => sized_root
                .child(
                    div()
                        .size_full()
                        .overflow_hidden()
                        .when(is_windowed && is_maximized_or_fullscreen, |d| {
                            d.rounded_bl(corner_r).rounded_br(corner_r)
                        })
                        .child(content_col),
                )
                .when_some(handle_el, |this, h| {
                    this.child(
                        div()
                            .absolute()
                            .top(-card_gap)
                            .left_0()
                            .right_0()
                            .h(card_gap)
                            .child(h),
                    )
                }),
            Some(DockResizeEdge::Bottom) => sized_root
                .child(
                    div()
                        .size_full()
                        .overflow_hidden()
                        .when(is_windowed && is_maximized_or_fullscreen, |d| {
                            d.rounded_bl(corner_r).rounded_br(corner_r)
                        })
                        .child(content_col),
                )
                .when_some(handle_el, |this, h| {
                    this.child(
                        div()
                            .absolute()
                            .bottom(-card_gap)
                            .left_0()
                            .right_0()
                            .h(card_gap)
                            .child(h),
                    )
                }),
            None => sized_root.child(content_col),
        };

        div()
            .id("dock-panel-root")
            .size_full()
            .child(final_el)
            .when_some(self.active_menu.clone(), |this, menu| this.child(menu))
    }
}

#[allow(dead_code)]
struct TabDragView {
    title: String,
    icon: AppIcon,
}

impl Render for TabDragView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);
        div()
            .flex()
            .items_center()
            .gap(SPACE_SM)
            .bg(surface_bg(t.bg_panel, cx))
            .border_1()
            .border_color(p.border_subtle)
            .rounded(RADIUS_STD)
            .px(SPACE_MD)
            .py(SPACE_XS)
            .child(
                self.icon
                    .size(ui_icon_std_ts(cx))
                    .text_color(p.text_secondary),
            )
            .child(self.title.clone())
    }
}
