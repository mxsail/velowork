use super::menu_item::*;
use super::popup_menu::*;
use crate::overlay_registry::OverlayRegistry;
use gpui::*;
use std::sync::Arc;

/// Helper for constructing dropdown/popover menus anchored to trigger bounds.
pub struct DropdownMenu;

impl DropdownMenu {
    /// Build a dropdown menu anchored to trigger bounds expanding below.
    pub fn build(
        cx: &mut App,
        trigger_bounds: Bounds<Pixels>,
        items: Vec<PopupMenuItem>,
        overlay_registry: Option<Entity<OverlayRegistry>>,
        on_close: Option<Arc<dyn Fn(&mut Window, &mut App) + Send + Sync>>,
    ) -> Entity<PopupMenu> {
        cx.new(|cx| {
            PopupMenu::new(cx, items, trigger_bounds.origin, overlay_registry, on_close)
                .trigger_bounds(trigger_bounds)
                .direction(PopupMenuDirection::Below)
        })
    }

    /// Build a dropdown menu expanding above the trigger bounds.
    pub fn build_above(
        cx: &mut App,
        trigger_bounds: Bounds<Pixels>,
        items: Vec<PopupMenuItem>,
        overlay_registry: Option<Entity<OverlayRegistry>>,
        on_close: Option<Arc<dyn Fn(&mut Window, &mut App) + Send + Sync>>,
    ) -> Entity<PopupMenu> {
        cx.new(|cx| {
            PopupMenu::new(cx, items, trigger_bounds.origin, overlay_registry, on_close)
                .trigger_bounds(trigger_bounds)
                .direction(PopupMenuDirection::Above)
        })
    }

    /// Build and open a dropdown menu on window expanding below.
    pub fn open(
        trigger_bounds: Bounds<Pixels>,
        items: Vec<PopupMenuItem>,
        overlay_registry: Option<Entity<OverlayRegistry>>,
        on_close: Option<Arc<dyn Fn(&mut Window, &mut App) + Send + Sync>>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<PopupMenu> {
        let menu = Self::build(cx, trigger_bounds, items, overlay_registry, on_close);
        let focus_handle = menu.read(cx).focus_handle.clone();
        window.focus(&focus_handle, cx);
        menu
    }

    /// Build and open a dropdown menu on window expanding above.
    pub fn open_above(
        trigger_bounds: Bounds<Pixels>,
        items: Vec<PopupMenuItem>,
        overlay_registry: Option<Entity<OverlayRegistry>>,
        on_close: Option<Arc<dyn Fn(&mut Window, &mut App) + Send + Sync>>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<PopupMenu> {
        let menu = Self::build_above(cx, trigger_bounds, items, overlay_registry, on_close);
        let focus_handle = menu.read(cx).focus_handle.clone();
        window.focus(&focus_handle, cx);
        menu
    }
}
