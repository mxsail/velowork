use super::menu_item::*;
use super::popup_menu::*;
use crate::overlay_registry::OverlayRegistry;
use gpui::*;
use std::sync::Arc;

/// Helper for constructing and opening context menus anchored at cursor coordinates.
pub struct ContextMenu;

impl ContextMenu {
    /// Instantiate a context menu entity at `position` with items and auto-registration in `OverlayRegistry`.
    pub fn build(
        cx: &mut App,
        position: Point<Pixels>,
        items: Vec<PopupMenuItem>,
        overlay_registry: Option<Entity<OverlayRegistry>>,
        on_close: Option<Arc<dyn Fn(&mut Window, &mut App) + Send + Sync>>,
    ) -> Entity<PopupMenu> {
        cx.new(|cx| PopupMenu::new(cx, items, position, overlay_registry, on_close))
    }

    /// Build and open a context menu, giving it initial focus on window and capturing
    /// the currently focused element as previous_focus for return on dismissal.
    pub fn open(
        position: Point<Pixels>,
        items: Vec<PopupMenuItem>,
        overlay_registry: Option<Entity<OverlayRegistry>>,
        on_close: Option<Arc<dyn Fn(&mut Window, &mut App) + Send + Sync>>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<PopupMenu> {
        let previous_focus = window.focused(cx);
        Self::open_with_origin(position, items, overlay_registry, on_close, previous_focus, window, cx)
    }

    /// Build and open a context menu with explicit previous_focus handle.
    pub fn open_with_origin(
        position: Point<Pixels>,
        items: Vec<PopupMenuItem>,
        overlay_registry: Option<Entity<OverlayRegistry>>,
        on_close: Option<Arc<dyn Fn(&mut Window, &mut App) + Send + Sync>>,
        previous_focus: Option<FocusHandle>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<PopupMenu> {
        let menu = Self::build(cx, position, items, overlay_registry, on_close);
        menu.update(cx, |m, _| m.set_previous_focus(previous_focus));
        let focus_handle = menu.read(cx).focus_handle.clone();
        window.focus(&focus_handle, cx);
        menu
    }
}
