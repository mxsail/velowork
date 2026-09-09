//! `FocusStore` — application-level focus authority.
//!
//! This is the top-level coordinator over per-window [`FocusManager`] entities
//! (built in the previous focus-management work). `FocusManager` owns the
//! physical focus *handle* bookkeeping for one window; `FocusStore` is the
//! app-global registry of those managers and the authority that records focus
//! *layer* transitions (Terminal / Dialog / Search / …) as a typed
//! `FocusEvent`.
//!
//! Components never push focus state on their own — they go through the store,
//! which delegates to the right per-window `FocusManager` and broadcasts the
//! layer change so any observer can react to "which category owns input now".

use crate::focus::{FocusLayer, FocusManager};
use gpui::*;
use std::collections::HashMap;
use velowork_state::WindowId;

/// Typed events for focus-layer transitions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FocusEvent {
    /// The focus layer changed (carries the new layer).
    LayerChanged(FocusLayer),
    /// A window became the OS-focused window (carries the window id).
    WindowFocused(WindowId),
}

impl EventEmitter<FocusEvent> for FocusStore {}

/// App-global registry of per-window focus managers + layer authority.
pub struct FocusStore {
    managers: HashMap<WindowId, Entity<FocusManager>>,
}

impl FocusStore {
    pub fn new() -> Self {
        Self {
            managers: HashMap::new(),
        }
    }

    /// Register (or replace) the focus manager for a window.
    pub fn register(&mut self, id: WindowId, manager: Entity<FocusManager>, _cx: &mut Context<Self>) {
        self.managers.insert(id, manager);
    }

    /// The focus manager for a window, if registered.
    pub fn manager(&self, id: WindowId) -> Option<Entity<FocusManager>> {
        self.managers.get(&id).cloned()
    }

    /// Current focus layer for a window (falls back to `None` if unknown).
    pub fn current_layer(&self, id: WindowId, cx: &App) -> FocusLayer {
        self.managers
            .get(&id)
            .map(|m| m.read(cx).current_layer())
            .unwrap_or(FocusLayer::None)
    }

    /// Request a direct, non-stacked focus-layer transition for a window.
    /// Delegates to the per-window `FocusManager` and broadcasts the change.
    pub fn request_layer(&mut self, id: WindowId, layer: FocusLayer, cx: &mut Context<Self>) {
        if let Some(mgr) = self.managers.get(&id) {
            mgr.update(cx, |m, cx| {
                m.request_focus(layer);
                cx.notify();
            });
            cx.emit(FocusEvent::LayerChanged(layer));
            cx.notify();
        }
    }

    /// Push a focus layer (e.g. Dialog over Terminal) for a window.
    pub fn push_layer(&mut self, id: WindowId, layer: FocusLayer, cx: &mut Context<Self>) {
        if let Some(mgr) = self.managers.get(&id) {
            mgr.update(cx, |m, cx| {
                m.push_layer(layer);
                cx.notify();
            });
            cx.emit(FocusEvent::LayerChanged(layer));
            cx.notify();
        }
    }

    /// Pop the most recent focus layer for a window.
    pub fn pop_layer(&mut self, id: WindowId, cx: &mut Context<Self>) {
        if let Some(mgr) = self.managers.get(&id) {
            let layer = mgr.update(cx, |m, cx| {
                let l = m.pop_layer();
                cx.notify();
                l
            });
            cx.emit(FocusEvent::LayerChanged(layer));
            cx.notify();
        }
    }

    /// Record which window is OS-focused.
    pub fn set_window_focused(&mut self, id: WindowId, cx: &mut Context<Self>) {
        cx.emit(FocusEvent::WindowFocused(id));
        cx.notify();
    }
}

/// Global handle for crate-level access.
#[derive(Clone)]
pub struct GlobalFocusStore(pub Entity<FocusStore>);

impl Global for GlobalFocusStore {}
