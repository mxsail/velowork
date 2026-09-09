//! `WindowStore` — owns per-window metadata (bounds, active, focused).
//!
//! Window lifecycle/geometry used to be implicit (scattered `WindowState` in
//! `WorkspaceData` and ad-hoc focus tracking). This store centralizes it as its
//! own domain with a typed `WindowEvent`.

use gpui::*;
use std::collections::HashMap;
use velowork_state::{WindowBounds, WindowId};

/// Per-window metadata tracked by the store.
#[derive(Clone, Debug)]
pub struct WindowEntry {
    pub id: WindowId,
    pub bounds: WindowBounds,
    pub is_active: bool,
    pub is_focused: bool,
}

/// Typed events for window-state changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindowEvent {
    Opened(WindowId),
    Closed(WindowId),
    Focused(WindowId),
    BoundsChanged(WindowId),
}

impl EventEmitter<WindowEvent> for WindowStore {}

/// Single-source owner of per-window metadata.
pub struct WindowStore {
    windows: HashMap<WindowId, WindowEntry>,
}

impl WindowStore {
    pub fn new() -> Self {
        Self {
            windows: HashMap::new(),
        }
    }

    pub fn register(&mut self, id: WindowId, bounds: WindowBounds, cx: &mut Context<Self>) {
        self.windows.insert(
            id,
            WindowEntry {
                id,
                bounds,
                is_active: true,
                is_focused: false,
            },
        );
        cx.emit(WindowEvent::Opened(id));
        cx.notify();
    }

    pub fn set_focused(&mut self, id: WindowId, focused: bool, cx: &mut Context<Self>) {
        if let Some(entry) = self.windows.get_mut(&id) {
            entry.is_focused = focused;
            cx.emit(WindowEvent::Focused(id));
            cx.notify();
        }
    }

    pub fn set_bounds(&mut self, id: WindowId, bounds: WindowBounds, cx: &mut Context<Self>) {
        if let Some(entry) = self.windows.get_mut(&id) {
            entry.bounds = bounds;
            cx.emit(WindowEvent::BoundsChanged(id));
            cx.notify();
        }
    }

    pub fn close(&mut self, id: WindowId, cx: &mut Context<Self>) {
        self.windows.remove(&id);
        cx.emit(WindowEvent::Closed(id));
        cx.notify();
    }

    pub fn entry(&self, id: WindowId) -> Option<&WindowEntry> {
        self.windows.get(&id)
    }

    pub fn active(&self) -> Option<&WindowEntry> {
        self.windows.values().find(|e| e.is_active)
    }
}

/// Global handle for crate-level access.
#[derive(Clone)]
pub struct GlobalWindowStore(pub Entity<WindowStore>);

impl Global for GlobalWindowStore {}
