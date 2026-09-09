//! Centralized overlay registry.
//!
//! Every transient floating UI surface (Dropdown, Popup, Tooltip, ContextMenu,
//! Modal, Toast, Palette) registers itself here with its on-screen `Bounds`
//! and a `ClosePolicy`. A single window-level `MouseDown` handler then asks
//! the registry whether the click landed inside any overlay; if not, every
//! `ClickOutside` overlay is closed.
//!
//! This replaces the previous per-overlay "transparent full-screen backdrop
//! with `on_mouse_down`" pattern, which only covered the overlay's own
//! layout container and therefore failed to close menus when the user clicked
//! outside that container (e.g. in a different dock/panel).

use gpui::*;
use std::sync::Arc;

/// How an overlay should be dismissed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ClosePolicy {
    /// Never auto-close; caller manages lifetime explicitly.
    #[default]
    Manual,
    /// Close when a `MouseDown` occurs outside the overlay's bounds.
    ClickOutside,
    /// Close when the overlay (or its anchor) loses focus.
    FocusLost,
    /// Close when the owning window is deactivated.
    WindowDeactivate,
    /// Close on the `Escape` key.
    Escape,
}

/// Stable identifier for a registered overlay (one per open surface).
pub type OverlayId = SharedString;

/// Bookkeeping for one open overlay surface.
#[derive(Clone)]
pub struct OverlayInfo {
    pub id: OverlayId,
    pub bounds: Bounds<Pixels>,
    pub secondary_bounds: Option<Bounds<Pixels>>,
    pub close_policy: ClosePolicy,
    pub z_index: i32,
}

impl OverlayInfo {
    /// Whether `point` falls within this overlay's rendered bounds.
    pub fn contains(&self, point: &Point<Pixels>) -> bool {
        let check = |b: &Bounds<Pixels>| {
            point.x >= b.origin.x
                && point.x <= b.origin.x + b.size.width
                && point.y >= b.origin.y
                && point.y <= b.origin.y + b.size.height
        };
        if check(&self.bounds) {
            return true;
        }
        if let Some(ref sec) = self.secondary_bounds {
            if check(sec) {
                return true;
            }
        }
        false
    }
}

type CloseFn = Arc<dyn Fn(&mut Window, &mut App) + Send + Sync>;

/// Registry of currently-open overlays for a single window.
///
/// Owned by the app-level `OverlayManager`; populated by individual overlay
/// views (which live in lower layers and only hold a `WeakEntity` handle to
/// this registry). The window routes `MouseDown` here so no dock/panel ever
/// needs to know about overlays.
pub struct OverlayRegistry {
    overlays: Vec<(OverlayInfo, CloseFn)>,
}

impl OverlayRegistry {
    pub fn new() -> Self {
        Self {
            overlays: Vec::new(),
        }
    }

    /// Register an overlay. If an id already exists it is replaced.
    pub fn register(
        &mut self,
        info: OverlayInfo,
        close: CloseFn,
    ) {
        self.unregister(&info.id);
        self.overlays.push((info, close));
    }

    /// Remove an overlay by id (no-op if absent).
    pub fn unregister(&mut self, id: &OverlayId) {
        self.overlays.retain(|(info, _)| &info.id != id);
    }

    /// Update the cached on-screen bounds for an overlay.
    pub fn set_bounds(&mut self, id: &OverlayId, bounds: Bounds<Pixels>) {
        if let Some((info, _)) = self.overlays.iter_mut().find(|(info, _)| &info.id == id) {
            info.bounds = bounds;
        }
    }

    /// Update cached secondary bounds (e.g. for open submenu panels).
    pub fn set_secondary_bounds(&mut self, id: &OverlayId, bounds: Option<Bounds<Pixels>>) {
        if let Some((info, _)) = self.overlays.iter_mut().find(|(info, _)| &info.id == id) {
            info.secondary_bounds = bounds;
        }
    }

    /// Update the z-index for stacking/hit-test priority.
    pub fn set_z_index(&mut self, id: &OverlayId, z: i32) {
        if let Some((info, _)) = self.overlays.iter_mut().find(|(info, _)| &info.id == id) {
            info.z_index = z;
        }
    }

    /// Topmost overlay whose bounds contain `point`, if any.
    pub fn topmost_at(&self, point: &Point<Pixels>) -> Option<OverlayId> {
        self.overlays
            .iter()
            .filter(|(info, _)| info.contains(point))
            .max_by_key(|(info, _)| info.z_index)
            .map(|(info, _)| info.id.clone())
    }

    /// Handle a window-level `MouseDown`.
    ///
    /// Any `ClickOutside` overlay whose bounds do NOT contain `point` is
    /// closed (its close callback invoked after it is unregistered, so the
    /// callback may safely re-enter the registry).
    pub fn handle_mouse_down(
        &mut self,
        point: Point<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let mut to_close: Vec<(OverlayId, CloseFn)> = Vec::new();
        for (info, close) in &self.overlays {
            if info.close_policy == ClosePolicy::ClickOutside && !info.contains(&point) {
                to_close.push((info.id.clone(), close.clone()));
            }
        }
        for (id, close) in to_close {
            self.unregister(&id);
            // Defer close callbacks to avoid re-entrancy: the caller may
            // already hold an `update` lock on the entity that registered
            // this overlay, and invoking the callback synchronously would
            // attempt a nested `update` on the same entity → panic.
            window.defer(cx, move |window, cx| {
                close(window, cx);
            });
        }
    }

    /// Force close all registered overlays immediately.
    pub fn close_all(
        &mut self,
        window: &mut Window,
        cx: &mut App,
    ) {
        let to_close: Vec<(OverlayInfo, CloseFn)> = self.overlays.drain(..).collect();
        for (_, close) in to_close {
            window.defer(cx, move |window, cx| {
                close(window, cx);
            });
        }
    }

    /// Force close all registered overlays whose z-index is less than or equal to `max_z_index`,
    /// except `except_id`. This prevents lower/same tier popovers from staying open while
    /// preserving higher-tier overlays like modal dialogs.
    pub fn close_others(
        &mut self,
        except_id: &OverlayId,
        max_z_index: i32,
        window: &mut Window,
        cx: &mut App,
    ) {
        let mut to_close = Vec::new();
        self.overlays.retain(|(info, close)| {
            if &info.id != except_id && info.z_index <= max_z_index {
                to_close.push(close.clone());
                false
            } else {
                true
            }
        });
        for close in to_close {
            window.defer(cx, move |window, cx| {
                close(window, cx);
            });
        }
    }
}

impl Default for OverlayRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Global wrapper allowing any UI component to automatically obtain the window's [`OverlayRegistry`].
pub struct GlobalOverlayRegistry(pub Entity<OverlayRegistry>);

impl Global for GlobalOverlayRegistry {}

impl OverlayRegistry {
    /// Retrieve the global `Entity<OverlayRegistry>` from `cx` if present.
    pub fn global(cx: &App) -> Option<Entity<OverlayRegistry>> {
        cx.try_global::<GlobalOverlayRegistry>().map(|g| g.0.clone())
    }

    /// Set `registry` as the global overlay registry for the current window context.
    pub fn set_global(registry: Entity<Self>, cx: &mut App) {
        cx.set_global(GlobalOverlayRegistry(registry));
    }
}

/// No-op close callback (used when an overlay has no explicit close action).
pub fn noop_close() -> CloseFn {
    Arc::new(|_, _| {})
}
