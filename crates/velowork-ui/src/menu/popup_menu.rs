use super::menu_item::*;
use crate::Cancel;
use crate::design::semantic::SemanticPalette;
use crate::icon::AppIcon;
use crate::overlay_registry::{ClosePolicy, OverlayInfo, OverlayRegistry};
use crate::theme::theme;
use crate::tokens::*;
use crate::h_flex;
use gpui::prelude::*;
use gpui::*;
use std::collections::HashMap;
use std::sync::Arc;

pub enum PopupMenuDirection {
    Below,
    Above,
}

/// Unified, data-driven Popup Menu entity with accessibility, keyboard navigation,
/// submenu support, and encapsulated `OverlayRegistry` integration.
pub struct PopupMenu {
    pub focus_handle: FocusHandle,
    pub previous_focus_handle: Option<FocusHandle>,
    items: Vec<PopupMenuItem>,
    selected_index: Option<usize>,
    on_close: Option<Arc<dyn Fn(&mut Window, &mut App) + Send + Sync>>,
    on_close_app: Option<Arc<dyn Fn(&mut App) + Send + Sync>>,
    trigger_position: Point<Pixels>,
    trigger_bounds: Option<Bounds<Pixels>>,
    direction: PopupMenuDirection,
    min_width: Option<Pixels>,
    width: Option<Pixels>,
    submenu_open: Option<SharedString>,
    main_menu_bounds: Option<Bounds<Pixels>>,
    submenu_bounds: HashMap<SharedString, Bounds<Pixels>>,
    sub_panel_bounds: Option<Bounds<Pixels>>,
    overlay_registry: Option<WeakEntity<OverlayRegistry>>,
    pub overlay_id: SharedString,
    auto_close_on_hover_out: bool,
    is_menu_hovered: bool,
    is_trigger_hovered: bool,
    hover_epoch: u64,
}

impl PopupMenu {
    /// Create a new PopupMenu entity anchored at a specific window position or trigger bounds.
    pub fn new(
        cx: &mut Context<Self>,
        items: Vec<PopupMenuItem>,
        position: Point<Pixels>,
        overlay_registry: Option<Entity<OverlayRegistry>>,
        on_close: Option<Arc<dyn Fn(&mut Window, &mut App) + Send + Sync>>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        let overlay_id = SharedString::from(format!("popup-menu-{}", cx.entity_id().as_u64()));

        let overlay_registry_weak = overlay_registry.map(|e| e.downgrade());

        if let Some(reg) = overlay_registry_weak.as_ref().and_then(|w| w.upgrade()) {
            let close_fn = on_close.clone();
            let id = overlay_id.clone();
            let this_weak = cx.entity().downgrade();

            let close: Arc<dyn Fn(&mut Window, &mut App) + Send + Sync> = Arc::new(move |window, cx| {
                if let Some(this) = this_weak.upgrade() {
                    this.update(cx, |this, cx| {
                        this.close(window, cx);
                    });
                } else if let Some(close_fn) = close_fn.as_ref() {
                    close_fn(window, cx);
                }
            });

            let init_bounds = Bounds::new(
                position,
                Size {
                    width: px(240.0),
                    height: px(350.0),
                },
            );

            reg.update(cx, |r, _| {
                r.register(
                    OverlayInfo {
                        id,
                        bounds: init_bounds,
                        secondary_bounds: None,
                        close_policy: ClosePolicy::ClickOutside,
                        z_index: 1000,
                    },
                    close,
                );
            });
        }

        Self {
            focus_handle,
            previous_focus_handle: None,
            items,
            selected_index: None,
            on_close,
            on_close_app: None,
            trigger_position: position,
            trigger_bounds: None,
            direction: PopupMenuDirection::Below,
            min_width: None,
            width: None,
            submenu_open: None,
            main_menu_bounds: None,
            submenu_bounds: HashMap::new(),
            sub_panel_bounds: None,
            overlay_registry: overlay_registry_weak,
            overlay_id,
            auto_close_on_hover_out: false,
            is_menu_hovered: false,
            is_trigger_hovered: false,
            hover_epoch: 0,
        }
    }

    /// Set the previous focus handle to restore when this popup menu closes.
    pub fn with_previous_focus(mut self, handle: Option<FocusHandle>) -> Self {
        self.previous_focus_handle = handle;
        self
    }

    /// Dynamically update or set the previous focus handle.
    pub fn set_previous_focus(&mut self, handle: Option<FocusHandle>) {
        self.previous_focus_handle = handle;
    }

    /// Enable or disable auto closing the menu when mouse leaves both trigger and menu panel.
    pub fn auto_close_on_hover_out(mut self, enabled: bool) -> Self {
        self.auto_close_on_hover_out = enabled;
        self
    }

    /// Set bounds of the trigger element if anchored to a button/widget.
    pub fn trigger_bounds(mut self, bounds: Bounds<Pixels>) -> Self {
        self.trigger_bounds = Some(bounds);
        self
    }

    /// Set expansion direction relative to trigger element.
    pub fn direction(mut self, direction: PopupMenuDirection) -> Self {
        self.direction = direction;
        self
    }

    /// Set explicit minimum width.
    pub fn min_width(mut self, min_width: Pixels) -> Self {
        self.min_width = Some(min_width);
        self
    }

    /// Set explicit width.
    pub fn width(mut self, width: Pixels) -> Self {
        self.width = Some(width);
        self
    }

    /// Dynamically update or set the on_close callback.
    pub fn set_on_close(&mut self, on_close: Option<Arc<dyn Fn(&mut Window, &mut App) + Send + Sync>>) {
        self.on_close = on_close;
    }

    /// Dynamically update or set the on_close_app callback.
    pub fn set_on_close_app(&mut self, on_close_app: Option<Arc<dyn Fn(&mut App) + Send + Sync>>) {
        self.on_close_app = on_close_app;
    }

    /// Notify popup menu about the hover status of its trigger element.
    pub fn set_trigger_hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if self.is_trigger_hovered != hovered {
            self.is_trigger_hovered = hovered;
            self.hover_epoch = self.hover_epoch.wrapping_add(1);
            if !hovered {
                self.schedule_hover_out_check(cx);
            }
        }
    }

    /// Schedule an async check to close the menu if still unhovered after 180ms debounce.
    pub fn schedule_hover_out_check(&mut self, cx: &mut Context<Self>) {
        if !self.auto_close_on_hover_out || self.is_menu_hovered || self.is_trigger_hovered {
            return;
        }
        let epoch = self.hover_epoch;
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(180))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.hover_epoch == epoch
                    && !this.is_menu_hovered
                    && !this.is_trigger_hovered
                    && this.auto_close_on_hover_out
                {
                    this.close_app(cx);
                }
            });
        })
        .detach();
    }

    /// Close and unregister this popup menu using application context.
    pub fn close_app(&mut self, cx: &mut Context<Self>) {
        if let Some(reg) = self.overlay_registry.as_ref().and_then(|w| w.upgrade()) {
            reg.update(cx, |r, _| {
                r.unregister(&self.overlay_id);
            });
        }

        if let Some(ref close_fn) = self.on_close_app {
            close_fn(cx);
        }

        cx.notify();
    }

    /// Close and unregister this popup menu.
    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(reg) = self.overlay_registry.as_ref().and_then(|w| w.upgrade()) {
            reg.update(cx, |r, _| {
                r.unregister(&self.overlay_id);
            });
        }

        if let Some(ref close_fn) = self.on_close {
            close_fn(window, cx);
        } else if let Some(ref close_fn) = self.on_close_app {
            close_fn(cx);
        }

        if let Some(ref prev) = self.previous_focus_handle {
            if window.focused(cx).map_or(true, |f| f == self.focus_handle) {
                window.focus(prev, cx);
            }
        }

        cx.notify();
    }

    /// Keyboard navigation handler.
    fn handle_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let selectable_indices: Vec<usize> = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(ix, item)| match item {
                PopupMenuItem::Item(data) if !data.disabled => Some(ix),
                PopupMenuItem::Submenu(data) if !data.disabled => Some(ix),
                _ => None,
            })
            .collect();

        if selectable_indices.is_empty() {
            if event.keystroke.key == "escape" {
                self.close(window, cx);
                cx.stop_propagation();
            }
            return;
        }

        match event.keystroke.key.as_str() {
            "down" => {
                let next_pos = match self.selected_index {
                    Some(cur) => {
                        let current_pos = selectable_indices
                            .iter()
                            .position(|&ix| ix == cur)
                            .unwrap_or(0);
                        (current_pos + 1) % selectable_indices.len()
                    }
                    None => 0,
                };
                self.selected_index = Some(selectable_indices[next_pos]);
                self.submenu_open = None;
                cx.notify();
                cx.stop_propagation();
            }
            "up" => {
                let next_pos = match self.selected_index {
                    Some(cur) => {
                        let current_pos = selectable_indices
                            .iter()
                            .position(|&ix| ix == cur)
                            .unwrap_or(0);
                        if current_pos == 0 {
                            selectable_indices.len() - 1
                        } else {
                            current_pos - 1
                        }
                    }
                    None => selectable_indices.len() - 1,
                };
                self.selected_index = Some(selectable_indices[next_pos]);
                self.submenu_open = None;
                cx.notify();
                cx.stop_propagation();
            }
            "right" => {
                if let Some(idx) = self.selected_index {
                    if let Some(PopupMenuItem::Submenu(sub)) = self.items.get(idx) {
                        if !sub.disabled {
                            self.submenu_open = Some(sub.id.clone());
                            cx.notify();
                            cx.stop_propagation();
                        }
                    }
                }
            }
            "left" => {
                if self.submenu_open.is_some() {
                    self.submenu_open = None;
                    cx.notify();
                    cx.stop_propagation();
                }
            }
            "enter" => {
                if let Some(idx) = self.selected_index {
                    if let Some(item) = self.items.get(idx) {
                        match item {
                            PopupMenuItem::Item(data) => {
                                if !data.disabled {
                                    let action = data.action.clone();
                                    self.close(window, cx);
                                    action(window, cx);
                                }
                            }
                            PopupMenuItem::Submenu(sub) => {
                                if !sub.disabled {
                                    self.submenu_open = Some(sub.id.clone());
                                    cx.notify();
                                }
                            }
                            _ => {}
                        }
                        cx.stop_propagation();
                    }
                }
            }
            "escape" => {
                self.close(window, cx);
                cx.stop_propagation();
            }
            _ => {}
        }
    }

    /// Render a nested submenu panel.
    fn render_submenu_panel(
        items: &[PopupMenuItem],
        on_close: &Option<Arc<dyn Fn(&mut Window, &mut App) + Send + Sync>>,
        text_size: Pixels,
        t: &crate::theme::ThemeColors,
        sp: &SemanticPalette,
        cx: &App,
    ) -> Div {
        let bg = sp.surface_overlay;
        let hover_bg = sp.surface_hover;

        let mut panel = div()
            .occlude()
            .bg(bg)
            .border_1()
            .border_color(sp.border_subtle)
            .rounded(RADIUS_MD)
            .shadow(elevation_menu_shadow())
            .min_w(px(160.0))
            .py(SPACE_XS)
            .px(SPACE_SM)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_mouse_move(|_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation());

        for item in items {
            match item {
                PopupMenuItem::Separator => {
                    panel = panel.child(div().h(px(1.0)).my(SPACE_XS).bg(sp.border_subtle));
                }
                PopupMenuItem::Label(text) => {
                    panel = panel.child(
                        div()
                            .px(SPACE_LG)
                            .py(SPACE_XS)
                            .text_size(ui_text_xs(cx))
                            .text_color(sp.text_muted)
                            .child(text.clone()),
                    );
                }
                PopupMenuItem::Item(data) => {
                    let action = data.action.clone();
                    let close = on_close.clone();
                    let disabled = data.disabled;

                    let mut row = div()
                        .id(SharedString::from(format!("submenu-item-{}", data.id)))
                        .w_full()
                        .px(SPACE_MD)
                        .py(SPACE_SM)
                        .rounded(RADIUS_SM)
                        .flex()
                        .items_center()
                        .text_size(text_size);

                    if disabled {
                        row = row.text_color(sp.text_muted);
                    } else {
                        row = row
                            .cursor_pointer()
                            .text_color(rgb(data.text_color.unwrap_or(t.text_secondary)))
                            .hover(move |s| s.bg(hover_bg).text_color(rgb(t.text_primary)))
                            .on_click(move |_, window, cx| {
                                cx.stop_propagation();
                                if let Some(ref close_fn) = close {
                                    close_fn(window, cx);
                                }
                                action(window, cx);
                            });
                    }

                    row = row.child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .items_center()
                            .gap(SPACE_MD)
                            .when_some(data.icon, |this, icon| {
                                this.child(
                                    icon.size(ui_icon_md(cx)).text_color(rgb(
                                        data.icon_color.unwrap_or(t.text_muted),
                                    )),
                                )
                            })
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .child(data.label.clone()),
                            )
                            .when_some(data.shortcut.clone(), |this, sc| {
                                this.child(
                                    div()
                                        .text_size(ui_text_xs(cx))
                                        .text_color(sp.text_muted)
                                        .child(sc),
                                )
                            })
                            .when_some(data.checked, |this, checked| {
                                this.child(if checked {
                                    AppIcon::Check
                                        .size(ui_icon_md(cx))
                                        .text_color(sp.surface_accent)
                                        .into_any_element()
                                } else {
                                    div().size(ui_icon_md(cx)).into_any_element()
                                })
                            }),
                    );

                    panel = panel.child(row);
                }
                PopupMenuItem::Submenu(_) | PopupMenuItem::Custom(_) => {}
            }
        }

        panel
    }
}

impl Render for PopupMenu {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let sp = SemanticPalette::from_context(cx);

        // 动态计算菜单最大高度：基于视口高度留出上下边距，避免固定上限导致选项被滚动隐藏。
        let viewport_h = window.viewport_size().height;
        let max_h = (viewport_h - SPACE_LG * 4.0).max(px(200.0));

        let this_weak = cx.entity().downgrade();
        let registry_for_bounds = self.overlay_registry.clone();
        let id_for_bounds = self.overlay_id.clone();
        let trigger_bounds_for_reg = self.trigger_bounds;
        let bounds_setter = move |bounds: Bounds<Pixels>, _window: &mut Window, cx: &mut App| {
            if let Some(reg) = registry_for_bounds.as_ref().and_then(|w| w.upgrade()) {
                reg.update(cx, |r, _| {
                    r.set_bounds(&id_for_bounds, bounds);
                    if let Some(tb) = trigger_bounds_for_reg {
                        r.set_secondary_bounds(&id_for_bounds, Some(tb));
                    }
                });
            }
            if let Some(this) = this_weak.upgrade() {
                let _ = this.update(cx, |this, _| {
                    this.main_menu_bounds = Some(bounds);
                });
            }
        };

        let (menu_pos, anchor) = if let Some(b) = self.trigger_bounds {
            match self.direction {
                PopupMenuDirection::Below => (
                    point(b.origin.x, b.origin.y + b.size.height + px(2.0)),
                    gpui::Anchor::TopLeft,
                ),
                PopupMenuDirection::Above => (
                    point(b.origin.x, b.origin.y - px(2.0)),
                    gpui::Anchor::BottomLeft,
                ),
            }
        } else {
            (self.trigger_position, gpui::Anchor::TopLeft)
        };

        let mut items_container = div()
            .id("popup-menu-items")
            .w_full()
            .flex()
            .flex_col()
            .max_h(max_h - px(8.0))
            .overflow_y_scroll();

        for (ix, item) in self.items.iter().enumerate() {
            let is_selected = self.selected_index == Some(ix);

            match item {
                PopupMenuItem::Separator => {
                    items_container = items_container.child(div().h(px(1.0)).my(SPACE_XS).bg(sp.border_subtle));
                }
                PopupMenuItem::Label(text) => {
                    items_container = items_container.child(
                        div()
                            .px(SPACE_MD)
                            .py(SPACE_XS)
                            .text_size(ui_text_xs(cx))
                            .text_color(sp.text_muted)
                            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                                if this.submenu_open.is_some() {
                                    this.submenu_open = None;
                                    cx.notify();
                                }
                            }))
                            .child(text.clone()),
                    );
                }
                PopupMenuItem::Custom(builder) => {
                    items_container = items_container.child(builder(window, cx));
                }
                PopupMenuItem::Item(data) => {
                    let action = data.action.clone();
                    let disabled = data.disabled;
                    let id = data.id.clone();

                    let mut row = div()
                        .id(SharedString::from(format!("popup-item-{}", id)))
                        .w_full()
                        .px(SPACE_MD)
                        .py(SPACE_SM)
                        .rounded(RADIUS_SM)
                        .flex()
                        .items_center()
                        .text_size(ui_text_md(cx));

                    if disabled {
                        row = row.text_color(sp.text_muted);
                    } else {
                        let item_bg = if is_selected {
                            sp.surface_hover
                        } else {
                            gpui::transparent_black()
                        };

                        row = row
                            .cursor_pointer()
                            .bg(item_bg)
                            .text_color(rgb(t.text_secondary))
                            .hover(move |s| s.bg(sp.surface_hover).text_color(rgb(t.text_primary)))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.close(window, cx);
                                action(window, cx);
                            }))
                            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                                if this.submenu_open.is_some() {
                                    this.submenu_open = None;
                                    cx.notify();
                                }
                            }));
                    }

                    row = row.child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .items_center()
                            .gap(SPACE_MD)
                            .when_some(data.icon, |this, icon| {
                                this.child(
                                    icon.size(ui_icon_md(cx)).text_color(rgb(
                                        data.icon_color.unwrap_or(t.text_muted),
                                    )),
                                )
                            })
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .child(data.label.clone()),
                            )
                            .when_some(data.shortcut.clone(), |this, sc| {
                                this.child(
                                    div()
                                        .text_size(ui_text_xs(cx))
                                        .text_color(sp.text_muted)
                                        .child(sc),
                                )
                            })
                            .when_some(data.checked, |this, checked| {
                                this.child(if checked {
                                    AppIcon::Check
                                        .size(ui_icon_md(cx))
                                        .text_color(sp.surface_accent)
                                        .into_any_element()
                                } else {
                                    div().size(ui_icon_md(cx)).into_any_element()
                                })
                            }),
                    );

                    items_container = items_container.child(row);
                }
                PopupMenuItem::Submenu(sub) => {
                    let is_open = self.submenu_open.as_ref() == Some(&sub.id);
                    let id = sub.id.clone();
                    let disabled = sub.disabled;
                    let this_weak_sub = cx.entity().downgrade();

                    let default_fg = if is_selected || is_open {
                        t.text_primary
                    } else {
                        t.text_secondary
                    };

                    let mut row = h_flex()
                        .id(SharedString::from(format!("popup-submenu-{}", id)))
                        .w_full()
                        .px(SPACE_MD)
                        .py(SPACE_SM)
                        .rounded(RADIUS_SM)
                        .items_center()
                        .justify_between()
                        .text_size(ui_text_md(cx));

                    if disabled {
                        row = row.text_color(sp.text_muted);
                    } else {
                        let is_highlighted = is_selected || is_open;
                        let item_bg = if is_highlighted {
                            sp.surface_hover
                        } else {
                            gpui::transparent_black()
                        };

                        row = row
                            .cursor_pointer()
                            .bg(item_bg)
                            .text_color(rgb(default_fg))
                            .hover(move |s| s.bg(sp.surface_hover).text_color(rgb(t.text_primary)))
                            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                                if this.submenu_open.as_ref() != Some(&id) {
                                    this.submenu_open = Some(id.clone());
                                    cx.notify();
                                }
                            }));
                    }

                    row = row
                        .when(!disabled, |d| {
                            let sub_id = sub.id.clone();
                            let w_sub = this_weak_sub.clone();
                            d.child(
                                canvas(
                                    move |bounds: Bounds<Pixels>, _window: &mut Window, cx: &mut App| {
                                        if let Some(this) = w_sub.upgrade() {
                                            let id_clone = sub_id.clone();
                                            let _ = this.update(cx, |this, _| {
                                                this.submenu_bounds.insert(id_clone, bounds);
                                            });
                                        }
                                    },
                                    |_, _, _, _| {},
                                )
                                .absolute()
                                .inset_0(),
                            )
                        })
                        .child(
                            h_flex()
                                .flex_1()
                                .min_w_0()
                                .items_center()
                                .gap(SPACE_MD)
                                .when_some(sub.icon, |this, icon| {
                                    this.child(
                                        icon.size(ui_icon_md(cx)).text_color(rgb(
                                            sub.icon_color.unwrap_or(t.text_muted),
                                        )),
                                    )
                                })
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .child(sub.label.clone()),
                                ),
                        )
                        .child(
                            AppIcon::ChevronRight
                                .size(ui_icon_sm(cx))
                                .text_color(sp.text_muted),
                        );

                    items_container = items_container.child(row);
                }
            }
        }

        let menu_panel = div()
            .id(SharedString::from(format!("popup-menu-{}", self.overlay_id)))
            .track_focus(&self.focus_handle)
            .occlude()
            .on_hover(cx.listener(|this, hovered: &bool, _window, cx| {
                if this.is_menu_hovered != *hovered {
                    this.is_menu_hovered = *hovered;
                    this.hover_epoch = this.hover_epoch.wrapping_add(1);
                    if !*hovered {
                        this.schedule_hover_out_check(cx);
                    }
                }
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_mouse_move(|_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .on_action(cx.listener(|this, _: &Cancel, window, cx| {
                this.close(window, cx);
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                this.handle_key_down(event, window, cx);
            }))
            .bg(sp.surface_overlay)
            .border_1()
            .border_color(sp.border_subtle)
            .rounded(RADIUS_MD)
            .shadow(elevation_menu_shadow())
            .when_some(self.min_width, |d, w| d.min_w(w))
            .when_none(&self.min_width, |d| d.min_w(px(140.0)))
            .when_some(self.width, |d, w| d.w(w))
            .p(SPACE_XS)
            .child(canvas(bounds_setter, |_, _, _, _| {}).absolute().inset_0())
            .child(items_container);

        let registry_for_sub = self.overlay_registry.clone();
        let id_for_sub = self.overlay_id.clone();
        let this_weak_sub_bounds = cx.entity().downgrade();

        let (sub_pos, sub_panel) = if let Some(ref open_id) = self.submenu_open {
            let item_bounds = self.submenu_bounds.get(open_id).cloned();
            let menu_bounds = self.main_menu_bounds.unwrap_or_else(|| Bounds::new(menu_pos, Size::default()));
            if let Some(item_bounds) = item_bounds {
                let items = self.items.iter().find_map(|item| {
                    if let PopupMenuItem::Submenu(sub) = item {
                        if sub.id == *open_id {
                            return Some(sub.items.clone());
                        }
                    }
                    None
                });
                if let Some(items) = items {
                    let on_close = self.on_close.clone();
                    let text_size = ui_text_md(cx);
                    let x = menu_bounds.origin.x + menu_bounds.size.width + px(2.0);
                    let y = item_bounds.origin.y - px(4.0);

                    let sub_bounds_setter = move |bounds: Bounds<Pixels>, _window: &mut Window, cx: &mut App| {
                        if let Some(reg) = registry_for_sub.as_ref().and_then(|w| w.upgrade()) {
                            reg.update(cx, |r, _| r.set_secondary_bounds(&id_for_sub, Some(bounds)));
                        }
                        if let Some(this) = this_weak_sub_bounds.upgrade() {
                            let _ = this.update(cx, |this, _| {
                                this.sub_panel_bounds = Some(bounds);
                            });
                        }
                    };

                    let panel_with_bounds = div()
                        .child(canvas(sub_bounds_setter, |_, _, _, _| {}).absolute().inset_0())
                        .child(Self::render_submenu_panel(&items, &on_close, text_size, &t, &sp, cx));

                    (
                        point(x, y),
                        panel_with_bounds.into_any_element(),
                    )
                } else {
                    (point(px(0.0), px(0.0)), div().invisible().size_0().into_any_element())
                }
            } else {
                (point(px(0.0), px(0.0)), div().invisible().size_0().into_any_element())
            }
        } else {
            if let Some(reg) = self.overlay_registry.as_ref().and_then(|w| w.upgrade()) {
                reg.update(cx, |r, _| r.set_secondary_bounds(&self.overlay_id, None));
            }
            (point(px(0.0), px(0.0)), div().invisible().size_0().into_any_element())
        };

        div()
            .child(
                deferred(
                    anchored()
                        .position(menu_pos)
                        .anchor(anchor)
                        .snap_to_window()
                        .child(menu_panel),
                ),
            )
            .child(
                deferred(
                    anchored()
                        .position(sub_pos)
                        .anchor(gpui::Anchor::TopLeft)
                        .snap_to_window()
                        .child(sub_panel),
                ),
            )
    }
}
