use crate::Cancel;
use crate::design::semantic::SemanticPalette;
use crate::overlay_registry::{ClosePolicy, OverlayInfo, OverlayRegistry};
use crate::icon::AppIcon;
use crate::theme::theme;
use crate::tokens::{ICON_MICRO, ICON_STD, RADIUS_MD, RADIUS_SM, RADIUS_STD, SPACE_2XS, SPACE_LG, SPACE_MD, SPACE_XS, ui_text_md, ui_text_xs};
use crate::tooltip::Tooltip;
use crate::h_flex;
use gpui::prelude::*;
use gpui::*;
use crate::behavior::{HoverBehavior, StatefulElementBehaviorExt};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A small right-aligned visual marker rendered inside a menu item.
/// Used to surface per-item status (e.g. a project with running terminals)
/// without altering the item's click behavior.
#[derive(Clone)]
pub enum OverlayMenuIndicator {
    /// A small filled status dot in the given (theme) color.
    Dot(u32),
    /// A small icon rendered on the right side.
    Icon(AppIcon),
}

#[derive(Clone)]
pub enum OverlayMenuEntry {
    Item(OverlayMenuItem),
    Separator,
    Footer(String),
    /// A toggle/checkbox row (e.g. "which panels to show").
    CheckItem(OverlayMenuCheckItem),
    /// A parent item that reveals a nested submenu on hover. The nested menu is
    /// rendered (anchored to the right of this row) by `OverlayMenu` itself when
    /// `submenu_open` matches `id`.
    Submenu(OverlayMenuSubmenu),
}

/// A right-aligned quick action button rendered on the trailing side of an item row.
#[derive(Clone)]
pub struct OverlayMenuAction {
    pub id: String,
    pub icon: AppIcon,
    pub tooltip: Option<String>,
    pub action: Arc<dyn Fn(&mut Window, &mut App) + Send + Sync>,
}

#[derive(Clone)]
pub struct OverlayMenuItem {
    pub id: String,
    pub icon: Option<AppIcon>,
    pub label: String,
    pub color_dot: Option<(u32, bool)>, // (color, hollow)
    pub shortcut: Option<String>,
    pub action: Arc<dyn Fn(&mut Window, &mut App) + Send + Sync>,
    pub trailing_actions: Vec<OverlayMenuAction>,
}

#[derive(Clone)]
pub struct OverlayMenuCheckItem {
    pub id: String,
    pub icon: Option<AppIcon>,
    pub label: String,
    /// Whether the checkbox is currently ticked.
    pub checked: bool,
    pub action: Arc<dyn Fn(&mut Window, &mut App) + Send + Sync>,
}

#[derive(Clone)]
pub struct OverlayMenuSubmenu {
    pub id: String,
    pub icon: Option<AppIcon>,
    pub label: String,
    /// Optional right-aligned shortcut hint (purely informational).
    pub shortcut: Option<String>,
    pub entries: Vec<OverlayMenuEntry>,
}

/// Which side of the trigger the menu expands toward.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum OverlayMenuDirection {
    /// Menu opens below the trigger (default). Top edge aligns under the
    /// button's bottom edge.
    Below,
    /// Menu opens above the trigger. Bottom edge aligns above the button's
    /// top edge — used for footer toolbars near the bottom of the window.
    Above,
}

/// Horizontal alignment of the menu relative to the trigger bounds.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum OverlayMenuAlign {
    /// Left-aligned with the trigger (menu expands to the right).
    #[default]
    Start,
    /// Horizontally centered relative to the trigger.
    Center,
    /// Right-aligned with the trigger (menu expands to the left).
    End,
}

pub struct OverlayMenu {
    pub focus_handle: FocusHandle,
    query: String,
    items: Vec<OverlayMenuEntry>,
    selected_index: usize,
    on_close: Option<Arc<dyn Fn(&mut Window, &mut App) + Send + Sync>>,
    trigger_bounds: Bounds<Pixels>,
    animation_progress: f32,
    /// Estimated height used on frame 0 to avoid white-frame lag.
    estimated_height: Pixels,
    /// Whether the open-animation spawn loop has been kicked off.
    animation_started: bool,
    /// 即时展开：跳过“从高度 0 生长”的开窗动画，首帧即以满高渲染，
    /// 用于需要“按下即弹出、零延迟”的工具栏菜单。
    instant_open: bool,
    search_placeholder: Option<String>,
    search_input: Option<Entity<crate::input::InputState>>,
    /// Registry this menu registers itself with for centralized click-outside dismissal.
    overlay_registry: Option<WeakEntity<OverlayRegistry>>,
    /// Stable id used to register/unregister in the `overlay_registry`.
    pub overlay_id: SharedString,
    /// Side of the trigger the menu expands toward.
    direction: OverlayMenuDirection,
    /// Horizontal alignment relative to the trigger bounds.
    align: OverlayMenuAlign,
    /// Optional minimum width applied to the menu surface.
    min_width: Option<Pixels>,
    /// Optional maximum width applied to the menu surface.
    max_width: Option<Pixels>,
    /// Per-item tooltips keyed by item id, shown when hovering the label.
    item_tooltips: HashMap<SharedString, String>,
    /// Id of the item representing the currently active/visible project.
    active_id: Option<String>,
    /// Per-item right-side status indicators keyed by item id.
    item_indicators: HashMap<SharedString, OverlayMenuIndicator>,
    /// Font size applied to item labels. Defaults to `ui_text_sm`.
    text_size: Pixels,
    /// Pointer interaction active indicator.
    mouse_active: bool,
    /// Index of the item currently hovered by the pointer, used to reveal
    /// per-item trailing actions (e.g. close buttons) only on hover.
    hovered_item: Option<usize>,
    /// Id of the submenu currently expanded on hover, if any.
    submenu_open: Option<String>,
    /// Bounds of the main menu panel surface.
    main_menu_bounds: Option<Bounds<Pixels>>,
    /// Bounds of the rows that triggered open submenus, keyed by submenu id.
    submenu_bounds: HashMap<String, Bounds<Pixels>>,
}

impl OverlayMenu {
    pub fn new(
        cx: &mut Context<Self>,
        items: Vec<OverlayMenuEntry>,
        search_placeholder: Option<String>,
        trigger_bounds: Bounds<Pixels>,
        on_close: Option<Arc<dyn Fn(&mut Window, &mut App) + Send + Sync>>,
        overlay_registry: Option<Entity<OverlayRegistry>>,
        overlay_id: SharedString,
    ) -> Self {
        let focus_handle = cx.focus_handle();

        // Calculate initial selected_index
        let selected_index = items
            .iter()
            .position(|entry| {
                matches!(
                    entry,
                    OverlayMenuEntry::Item(_) | OverlayMenuEntry::CheckItem(_)
                )
            })
            .unwrap_or(0);

        // 预估菜单自然高度（避免第 0 帧闪烁等待）：搜索框 (~48px) + 每个 item (~30px) + padding (~16px)
        let search_h = if search_placeholder.is_some() {
            48.0
        } else {
            0.0
        };
        let items_h = items.len() as f32 * 28.0;
        let estimated_height = px((search_h + items_h + 16.0).min(480.0));

        let overlay_registry = overlay_registry.map(|e| e.downgrade());
        if let Some(reg) = overlay_registry.clone().and_then(|w| w.upgrade()) {
            let close = on_close
                .clone()
                .unwrap_or_else(|| crate::overlay_registry::noop_close());
            let id = overlay_id.clone();
            let init_bounds = Bounds::new(
                point(trigger_bounds.origin.x, trigger_bounds.origin.y - px(330.0)),
                Size {
                    width: px(260.0).max(trigger_bounds.size.width),
                    height: px(f32::from(trigger_bounds.size.height) + 660.0),
                },
            );
            reg.update(cx, |r, _cx| {
                r.register(
                    OverlayInfo {
                        id,
                        bounds: init_bounds,
                        secondary_bounds: None,
                        close_policy: ClosePolicy::ClickOutside,
                        z_index: 1000,
                    },
                    close,
                )
            });
        }

        Self {
            focus_handle,
            query: String::new(),
            items,
            selected_index,
            on_close,
            trigger_bounds,
            animation_progress: 0.0,
            estimated_height,
            animation_started: false,
            instant_open: false,
            search_placeholder,
            search_input: None,
            overlay_registry,
            overlay_id,
            direction: OverlayMenuDirection::Below,
            align: OverlayMenuAlign::Start,
            min_width: None,
            max_width: None,
            item_tooltips: HashMap::new(),
            active_id: None,
            item_indicators: HashMap::new(),
            mouse_active: false,
            hovered_item: None,
            submenu_open: None,
            main_menu_bounds: None,
            submenu_bounds: HashMap::new(),
            text_size: ui_text_md(cx),
        }
    }

    pub fn with_direction(mut self, direction: OverlayMenuDirection) -> Self {
        self.direction = direction;
        self
    }

    pub fn with_align(mut self, align: OverlayMenuAlign) -> Self {
        self.align = align;
        self
    }

    pub fn with_min_width(mut self, min_width: Pixels) -> Self {
        self.min_width = Some(min_width);
        self
    }

    pub fn with_max_width(mut self, max_width: Pixels) -> Self {
        self.max_width = Some(max_width);
        self
    }

    pub fn with_item_tooltips(mut self, tooltips: HashMap<SharedString, String>) -> Self {
        self.item_tooltips = tooltips;
        self
    }

    pub fn with_active_id(mut self, active_id: Option<String>) -> Self {
        if let Some(ref id) = active_id {
            if let Some(idx) = self
                .items
                .iter()
                .position(|e| matches!(e, OverlayMenuEntry::Item(item) if &item.id == id))
            {
                self.selected_index = idx;
            }
        }
        self.active_id = active_id;
        self
    }

    pub fn with_item_indicators(
        mut self,
        indicators: HashMap<SharedString, OverlayMenuIndicator>,
    ) -> Self {
        self.item_indicators = indicators;
        self
    }

    pub fn with_text_size(mut self, size: Pixels) -> Self {
        self.text_size = size;
        self
    }

    /// 开启即时展开：跳过“从高度 0 生长”的开窗动画，首帧即以满高渲染。
    pub fn with_instant_open(mut self, instant: bool) -> Self {
        self.instant_open = instant;
        self
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        window.focus(&self.focus_handle, cx);
    }

    fn filtered_indices(&self) -> Vec<usize> {
        let query_lower = self.query.to_lowercase();
        let mut result = Vec::new();
        for (i, entry) in self.items.iter().enumerate() {
            let label = match entry {
                OverlayMenuEntry::Item(item) => item.label.clone(),
                OverlayMenuEntry::CheckItem(item) => item.label.clone(),
                OverlayMenuEntry::Submenu(sub) => sub.label.clone(),
                OverlayMenuEntry::Separator => {
                    // 分隔线在搜索时隐藏（除非有激活查询也无意义），无查询时始终可见
                    if query_lower.is_empty() {
                        result.push(i);
                    }
                    continue;
                }
                OverlayMenuEntry::Footer(_) => {
                    if query_lower.is_empty() {
                        result.push(i);
                    }
                    continue;
                }
            };
            if query_lower.is_empty() || label.to_lowercase().contains(&query_lower) {
                result.push(i);
            }
        }
        result
    }

    fn select_prev(&mut self, cx: &mut Context<Self>) {
        self.mouse_active = false;
        let filtered = self.filtered_indices();
        if filtered.is_empty() {
            return;
        }
        let current_pos = filtered.iter().position(|&i| i == self.selected_index);
        if let Some(pos) = current_pos {
            if pos > 0 {
                self.selected_index = filtered[pos - 1];
            } else {
                self.selected_index = *filtered.last().unwrap();
            }
        } else {
            self.selected_index = filtered[0];
        }
        cx.notify();
    }

    fn select_next(&mut self, cx: &mut Context<Self>) {
        self.mouse_active = false;
        let filtered = self.filtered_indices();
        if filtered.is_empty() {
            return;
        }
        let current_pos = filtered.iter().position(|&i| i == self.selected_index);
        if let Some(pos) = current_pos {
            if pos + 1 < filtered.len() {
                self.selected_index = filtered[pos + 1];
            } else {
                self.selected_index = filtered[0];
            }
        } else {
            self.selected_index = filtered[0];
        }
        cx.notify();
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let filtered = self.filtered_indices();
        if let Some(&idx) = filtered.iter().find(|&&i| i == self.selected_index) {
            let action = match &self.items[idx] {
                OverlayMenuEntry::Item(item) => item.action.clone(),
                OverlayMenuEntry::CheckItem(item) => item.action.clone(),
                _ => return,
            };
            self.close(window, cx);
            action(window, cx);
        }
    }

    pub fn set_on_close(&mut self, on_close: Option<Arc<dyn Fn(&mut Window, &mut App) + Send + Sync>>) {
        self.on_close = on_close;
    }

    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(reg) = self.overlay_registry.clone().and_then(|w| w.upgrade()) {
            reg.update(cx, |r, _| r.unregister(&self.overlay_id));
        }
        if let Some(ref on_close) = self.on_close {
            on_close(window, cx);
        }
    }

    fn handle_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.as_str();
        match key {
            "escape" => self.close(window, cx),
            "up" => self.select_prev(cx),
            "down" => self.select_next(cx),
            "enter" => self.confirm(window, cx),
            "backspace" => {
                if self.search_input.is_none() {
                    self.query.pop();
                    let filtered = self.filtered_indices();
                    if !filtered.is_empty() && !filtered.contains(&self.selected_index) {
                        self.selected_index = filtered[0];
                    }
                    cx.notify();
                }
            }
            key if key.len() == 1 => {
                if self.search_input.is_none() {
                    self.query.push_str(key);
                    let filtered = self.filtered_indices();
                    if !filtered.is_empty() && !filtered.contains(&self.selected_index) {
                        self.selected_index = filtered[0];
                    }
                    cx.notify();
                }
            }
            _ => {}
        }
    }
    /// Render a submenu panel as an inline element (no separate entity).
    ///
    /// This avoids the GPUI "node was not part of the reused subtree" panic
    /// that occurs when a nested `OverlayMenu` entity (with its own
    /// `FocusHandle`) is created during an event handler and then rendered
    /// as a child view.
    fn render_submenu_panel(
        entries: &[OverlayMenuEntry],
        on_close: &Option<Arc<dyn Fn(&mut Window, &mut App) + Send + Sync>>,
        text_size: Pixels,
        _t: &crate::theme::ThemeColors,
        sp: &SemanticPalette,
        cx: &App,
    ) -> Div {
        let bg = sp.surface_overlay;
        let hover_bg = sp.surface_hover;

        let mut panel = div()
            .occlude()
            .relative()
            .bg(bg)
            .border_1()
            .border_color(sp.border_subtle)
            .rounded(RADIUS_MD)
            .shadow(crate::tokens::elevation_menu_shadow())
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left(RADIUS_MD)
                    .right(RADIUS_MD)
                    .h(px(1.0))
                    .bg(white().opacity(0.08)),
            )
            .min_w(px(140.0))
            .p(SPACE_XS)
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            })
            .on_mouse_down(MouseButton::Right, |_, _, cx| {
                cx.stop_propagation();
            })
            .on_mouse_up(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            })
            .on_mouse_up(MouseButton::Right, |_, _, cx| {
                cx.stop_propagation();
            })
            .on_mouse_move(|_, _, cx| {
                cx.stop_propagation();
            })
            .on_scroll_wheel(|_, _, cx| {
                cx.stop_propagation();
            });

        for entry in entries {
            match entry {
                OverlayMenuEntry::Separator => {
                    panel = panel.child(
                        div()
                            .h(px(1.0))
                            .mx(SPACE_XS)
                            .my(SPACE_XS)
                            .bg(sp.border_subtle),
                    );
                }
                OverlayMenuEntry::Footer(text) => {
                    panel = panel.child(
                        div()
                            .px(SPACE_MD)
                            .py(SPACE_XS)
                            .text_size(ui_text_xs(cx))
                            .text_color(sp.text_muted)
                            .child(text.clone()),
                    );
                }
                OverlayMenuEntry::Item(item) => {
                    let item_h = crate::menu::menu_item_height(cx);
                    let action = item.action.clone();
                    let close = on_close.clone();
                    let mut row = div()
                        .id(SharedString::from(format!("submenu-item-{}", item.id)))
                        .w_full()
                        .h(item_h)
                        .px(SPACE_MD)
                        .rounded(RADIUS_SM)
                        .flex()
                        .justify_between()
                        .items_center()
                        .cursor_pointer()
                        .text_size(text_size)
                        .text_color(sp.text_primary)
                        .hover(move |s| s.bg(hover_bg))
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            if let Some(ref close_fn) = close {
                                close_fn(window, cx);
                            }
                            action(window, cx);
                        })
                        .child(
                            crate::h_flex()
                                .flex_1()
                                .min_w_0()
                                .items_center()
                                .gap(SPACE_MD)
                                .when_some(item.icon, |this, app_icon| {
                                    this.child(
                                        app_icon
                                            .size(px(15.0))
                                             .text_color(sp.text_muted),
                                    )
                                })
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .child(item.label.clone()),
                                ),
                        );

                    // Shortcut hint
                    if let Some(ref keys) = item.shortcut {
                        row = row.child(
                            div()
                                .flex_none()
                                .pl(SPACE_LG)
                                .text_size(ui_text_xs(cx))
                                .text_color(sp.text_muted)
                                .child(keys.clone()),
                        );
                    }

                    // Trailing actions (e.g. close button on tab list items)
                    if !item.trailing_actions.is_empty() {
                        row = row.child(
                            crate::h_flex()
                                .gap(SPACE_XS)
                                .items_center()
                                .children(item.trailing_actions.iter().map(|act| {
                                    let action = act.action.clone();
                                    let tip = act.tooltip.clone();
                                    let close_size = ui_text_md(cx) + px(4.0);
                                    div()
                                        .id(SharedString::from(format!("trailing-{}", act.id)))
                                        .flex_shrink_0()
                                        .w(close_size)
                                        .h(close_size)
                                        .rounded(RADIUS_SM)
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .cursor_pointer()
                                        .text_color(sp.text_muted)
                                        .hover(|s| s.bg(rgba(0xf14c4c99)))
                                        .when_some(tip, |this, tip| {
                                            this.tooltip(move |_, cx| {
                                                cx.new(|_| Tooltip::new(tip.clone())).into()
                                            })
                                        })
                                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation();
                                        })
                                        .on_click(move |_, window, cx| {
                                            cx.stop_propagation();
                                            (action)(window, cx);
                                        })
                                        .child(
                                            act.icon
                                                .size(ui_text_md(cx))
                                                .text_color(sp.text_muted)
                                                .into_any_element(),
                                        )
                                        .into_any_element()
                                })),
                        );
                    }

                    panel = panel.child(row);
                }
                OverlayMenuEntry::CheckItem(item) => {
                    let item_h = crate::menu::menu_item_height(cx);
                    let action = item.action.clone();
                    let close = on_close.clone();
                    let row = div()
                        .id(SharedString::from(format!("submenu-check-{}", item.id)))
                        .w_full()
                        .h(item_h)
                        .px(SPACE_MD)
                        .rounded(RADIUS_SM)
                        .flex()
                        .items_center()
                        .justify_between()
                        .cursor_pointer()
                        .text_size(text_size)
                        .text_color(sp.text_primary)
                        .hover(move |s| s.bg(hover_bg))
                        .on_click(move |_, window, cx| {
                            if let Some(ref close_fn) = close {
                                close_fn(window, cx);
                            }
                            action(window, cx);
                        })
                        .child(
                            crate::h_flex()
                                .flex_1()
                                .min_w_0()
                                .items_center()
                                .gap(SPACE_MD)
                                .when_some(item.icon, |this, app_icon| {
                                    this.child(
                                        app_icon
                                            .size(px(15.0))
                                            .text_color(sp.text_muted),
                                    )
                                })
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .child(item.label.clone()),
                                ),
                        )
                        .child(
                            if item.checked {
                                AppIcon::Check
                                    .size(px(15.0))
                                    .text_color(sp.surface_accent)
                                    .into_any_element()
                            } else {
                                div().size(px(15.0)).into_any_element()
                            },
                        );
                    panel = panel.child(row);
                }
                // Nested submenus inside a submenu are not supported (single level only)
                OverlayMenuEntry::Submenu(_) => {}
            }
        }

        panel
    }
}

impl Focusable for OverlayMenu {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for OverlayMenu {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let sp = SemanticPalette::from_context(cx);
        let this_weak = std::rc::Rc::new(cx.entity().downgrade());

        let viewport = window.viewport_size();
        let safe_margin = SPACE_MD; // 12px 面板安全留白，确保阴影 100% 收敛在窗口内部
        let menu_width = self.min_width.unwrap_or(px(260.0));
        let (mut pos, anchor) = match (self.direction, self.align) {
            (OverlayMenuDirection::Below, OverlayMenuAlign::Start) => (
                point(
                    self.trigger_bounds.origin.x,
                    self.trigger_bounds.origin.y + self.trigger_bounds.size.height + SPACE_XS,
                ),
                gpui::Anchor::TopLeft,
            ),
            (OverlayMenuDirection::Below, OverlayMenuAlign::End) => (
                point(
                    self.trigger_bounds.origin.x + self.trigger_bounds.size.width,
                    self.trigger_bounds.origin.y + self.trigger_bounds.size.height + SPACE_XS,
                ),
                gpui::Anchor::TopRight,
            ),
            (OverlayMenuDirection::Below, OverlayMenuAlign::Center) => (
                point(
                    self.trigger_bounds.origin.x + (self.trigger_bounds.size.width - menu_width) / 2.0,
                    self.trigger_bounds.origin.y + self.trigger_bounds.size.height + SPACE_XS,
                ),
                gpui::Anchor::TopLeft,
            ),
            (OverlayMenuDirection::Above, OverlayMenuAlign::Start) => (
                point(
                    self.trigger_bounds.origin.x,
                    self.trigger_bounds.origin.y - SPACE_XS,
                ),
                gpui::Anchor::BottomLeft,
            ),
            (OverlayMenuDirection::Above, OverlayMenuAlign::End) => (
                point(
                    self.trigger_bounds.origin.x + self.trigger_bounds.size.width,
                    self.trigger_bounds.origin.y - SPACE_XS,
                ),
                gpui::Anchor::BottomRight,
            ),
            (OverlayMenuDirection::Above, OverlayMenuAlign::Center) => (
                point(
                    self.trigger_bounds.origin.x + (self.trigger_bounds.size.width - menu_width) / 2.0,
                    self.trigger_bounds.origin.y - SPACE_XS,
                ),
                gpui::Anchor::BottomLeft,
            ),
        };

        // 视口安全边距钳制：消除紧贴窗口边缘现象，避免阴影穿透/外溢到主窗口之外
        match anchor {
            gpui::Anchor::TopRight | gpui::Anchor::BottomRight => {
                let max_right = (viewport.width - safe_margin).max(px(0.0));
                let min_right = (menu_width + safe_margin).min(max_right);
                pos.x = pos.x.clamp(min_right, max_right);
            }
            gpui::Anchor::TopLeft | gpui::Anchor::BottomLeft => {
                let min_left = safe_margin;
                let max_left = (viewport.width - menu_width - safe_margin).max(min_left);
                pos.x = pos.x.clamp(min_left, max_left);
            }
            _ => {}
        }

        let registry_for_bounds = self.overlay_registry.clone();
        let id_for_bounds = self.overlay_id.clone();
        let this_weak_for_bounds = cx.entity().downgrade();
        let bounds_setter = move |bounds: Bounds<Pixels>, _window: &mut Window, cx: &mut App| {
            if let Some(reg) = registry_for_bounds.as_ref().and_then(|w| w.upgrade()) {
                reg.update(cx, |r, _| r.set_bounds(&id_for_bounds, bounds));
            }
            if let Some(this) = this_weak_for_bounds.upgrade() {
                let _ = this.update(cx, |this, _| {
                    this.main_menu_bounds = Some(bounds);
                });
            }
        };

        let search_input = if let Some(ref placeholder) = self.search_placeholder {
            let input = self.search_input.get_or_insert_with(|| {
                let placeholder = placeholder.clone();
                let input = cx.new(|cx| {
                    crate::input::InputState::new(cx).placeholder(placeholder)
                });
                cx.subscribe(&input, move |this, input, event: &crate::input::InputEvent, cx| {
                    if let crate::input::InputEvent::Change = event {
                        this.query = input.read(cx).text().to_string();
                        let filtered = this.filtered_indices();
                        if !filtered.is_empty() {
                            this.selected_index = filtered[0];
                        }
                        cx.notify();
                    }
                })
                .detach();
                input
            });
            Some(input.clone())
        } else {
            None
        };

        // 启动基于真实的 80ms 极速时间差展开动画（纯对齐 VSync 刷新率）
        if !self.animation_started {
            if self.instant_open {
                // 即时展开：首帧即以满高渲染，跳过“从 0 生长”的动画，消除点击延迟。
                self.animation_progress = 1.0;
                self.animation_started = true;
            } else {
                self.animation_started = true;
                let start_time = Instant::now();
                let duration = Duration::from_millis(85); // 85ms 黄金流畅开窗体验

                cx.spawn(async move |this: WeakEntity<Self>, cx| {
                    loop {
                        let elapsed = start_time.elapsed();
                        let progress = (elapsed.as_secs_f32() / duration.as_secs_f32()).min(1.0);
                        let finished = progress >= 1.0;

                        let _ = this.update(cx, |this, cx| {
                            this.animation_progress = progress;
                            cx.notify();
                        });

                        if finished {
                            break;
                        }

                        // 120/144Hz 等高刷屏精确定位下一帧（约 7ms）
                        cx.background_executor()
                            .timer(Duration::from_millis(7))
                            .await;
                    }
                })
                .detach();
            }
        }

        let item_h = crate::menu::menu_item_height(cx);
        let p = self.animation_progress;

        // Quartic Ease-Out 缓动算法 (1 - (1 - t)^4)：
        // 在前 20% 的时间瞬间弹展开 60% 以上的高高度，末尾微快定格，带来媲美 macOS / Raycast 的清脆弹展感
        let inv = 1.0 - p.clamp(0.0, 1.0);
        let eased = 1.0 - inv * inv * inv * inv;

        // 目标真实高度（第 0 帧即用估算高度，确保零延迟展开且不闪烁）
        let target_height = self.estimated_height;
        let current_height = target_height * eased;

        // 快速透明度过度：前 35% 的动画时间内迅速淡入完成
        let opacity = (p * 2.85).min(1.0);

        // 主菜单与子菜单必须是平级兄弟 deferred，绝不可把 deferred 嵌套在另一个
        // deferred/anchored 子树内部——否则 GPUI 的 prepaint reuse 会错乱
        // （reuse_prepaint/refresh_node_id panic）。
        div()
            .child(
                deferred(
                    anchored()
                        .position(pos)
                        .anchor(anchor)
                        .snap_to_window()
                        .child(
                    div()
                        .occlude()
                        .when_some(self.min_width, |this, w| this.min_w(w))
                        .when_some(self.max_width, |this, w| this.max_w(w))
                        .max_h(if p < 1.0 {
                            current_height.min(px(480.0))
                        } else {
                            px(480.0)
                        })
                        .opacity(opacity)
                        .overflow_hidden()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                            cx.stop_propagation();
                        })
                        .on_scroll_wheel(|_, _, cx| {
                            cx.stop_propagation();
                        })
                        .track_focus(&self.focus_handle)
                        .on_action(cx.listener(|this, _: &Cancel, window, cx| {
                            this.close(window, cx);
                        }))
                        .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                            this.handle_key_down(event, window, cx);
                        }))
                        .flex()
                        .flex_col()
                        .bg(sp.surface_overlay)
                        .border_1()
                        .border_color(sp.border_subtle)
                        .rounded(RADIUS_MD)
                        .shadow(crate::tokens::elevation_menu_shadow())
                        .child(canvas(bounds_setter, |_, _, _, _| {}).absolute().inset_0())
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .left(RADIUS_MD)
                                .right(RADIUS_MD)
                                .h(px(1.0))
                                .bg(white().opacity(0.08)),
                        )
                        .when_some(search_input, |this, search_input| {
                            this.child(
                                div()
                                    .px(SPACE_XS)
                                    .pt(SPACE_XS)
                                    .pb(SPACE_XS)
                                    .child(
                                        crate::search_field::SearchField::new(&search_input, &t, cx),
                                    ),
                            )
                            .child(
                                div()
                                    .h(px(1.0))
                                    .mx(SPACE_XS)
                                    .my(SPACE_2XS)
                                    .bg(sp.border_subtle),
                            )
                        })
                        .child(
                            div()
                                .id("items-scroll-container")
                                .w_full()
                                .overflow_y_scroll()
                                .p(SPACE_XS)
                                .children(self.items.iter().enumerate().map(|(ix, entry)| {
                                    match entry {
                                        OverlayMenuEntry::Separator => {
                                            div()
                                                .id(format!("menu-separator-{}", ix))
                                                .h(px(1.0))
                                                .mx(SPACE_XS)
                                                .my(SPACE_XS)
                                                .bg(sp.border_subtle)
                                                .into_any_element()
                                        }
                                                OverlayMenuEntry::Footer(text) => {
                                                    div()
                                                        .id(format!("menu-footer-{}", ix))
                                                        .px(SPACE_MD)
                                                        .py(SPACE_XS)
                                                        .text_size(ui_text_xs(cx))
                                                        .text_color(sp.text_muted)
                                                        .child(text.clone())
                                                        .into_any_element()
                                                }
                                                OverlayMenuEntry::Submenu(sub) => {
                                                    let filtered = self.filtered_indices();
                                                    let is_visible = filtered.contains(&ix);
                                                    if !is_visible {
                                                        return div()
                                                            .id(format!("menu-hidden-{}", ix))
                                                            .h(px(0.0))
                                                            .w(px(0.0))
                                                            .into_any_element();
                                                    }

                                                    let is_open = self.submenu_open.as_deref() == Some(sub.id.as_str());
                                                    let is_hovered = self.hovered_item == Some(ix);
                                                    let is_selected = ix == self.selected_index;
                                                    let mouse_active = self.mouse_active;
                                                    let is_highlighted = is_open || (is_selected && !mouse_active) || is_hovered;
                                                    let id = sub.id.clone();

                                                    h_flex()
                                                        .id(format!("menu-submenu-{}", sub.id))
                                                        .w_full()
                                                        .h(item_h)
                                                        .px(SPACE_MD)
                                                        .rounded(RADIUS_SM)
                                                        .items_center()
                                                        .justify_between()
                                                        .cursor_pointer()
                                                        .when(is_open, |d| d.bg(sp.surface_hover))
                                                        .stateful_behavior(HoverBehavior { hover_bg: sp.surface_hover, ..Default::default() })
                                                        .on_hover(cx.listener(move |this, &hovered, _, cx| {
                                                            let next = if hovered { Some(ix) } else { None };
                                                            if this.hovered_item != next {
                                                                this.hovered_item = next;
                                                                cx.notify();
                                                            }
                                                        }))
                                                        .on_mouse_move(cx.listener(move |this, _phase: &MouseMoveEvent, _, cx| {
                                                            if this.submenu_open.as_deref() != Some(id.as_str()) {
                                                                 this.submenu_open = Some(id.clone());
                                                                cx.notify();
                                                            }
                                                        }))
                                                        .child({
                                                            let sub_id = sub.id.clone();
                                                            let submenu_weak = this_weak.clone();
                                                            canvas(
                                                                move |bounds: Bounds<Pixels>, _window: &mut Window, cx: &mut App| {
                                                                    let w = submenu_weak.clone();
                                                                    let id_clone = sub_id.clone();
                                                                    if let Some(ent) = w.upgrade() {
                                                                        ent.update(cx, |this, _| {
                                                                            this.submenu_bounds.insert(id_clone, bounds);
                                                                        });
                                                                    }
                                                                },
                                                                |_, _, _, _| {},
                                                            )
                                                            .absolute()
                                                            .inset_0()
                                                        })
                                                        .child(
                                                            h_flex()
                                                                .flex_1()
                                                                .min_w_0()
                                                                .items_center()
                                                                .gap(SPACE_MD)
                                                                .when_some(sub.icon, |this, app_icon| {
                                                                    this.child(
                                                                        app_icon
                                                                            .size(px(15.0))
                                                                            .text_color(if is_highlighted { sp.text_primary } else { sp.text_muted }),
                                                                    )
                                                                })
                                                                .child(
                                                                    div()
                                                                        .flex_1()
                                                                        .min_w_0()
                                                                        .pr(SPACE_XS)
                                                                        .text_size(self.text_size)
                                                                        .text_color(sp.text_primary)
                                                                        .truncate()
                                                                        .child(sub.label.clone()),
                                                                ),
                                                        )
                                                        .when_some(sub.shortcut.clone(), |this, keys| {
                                                            this.child(
                                                                div()
                                                                    .flex_none()
                                                                    .pl(SPACE_LG)
                                                                    .text_size(ui_text_xs(cx))
                                                                    .text_color(sp.text_muted)
                                                                    .child(keys),
                                                            )
                                                        })
                                                        .child(
                                                            AppIcon::ChevronRight
                                                                .size(ICON_MICRO)
                                                                .text_color(sp.text_muted),
                                                        )
                                                        .into_any_element()
                                                }
                                                OverlayMenuEntry::Item(item) => {
                                                    let filtered = self.filtered_indices();
                                                    let is_visible = filtered.contains(&ix);
                                                    if !is_visible {
                                                         return div()
                                                            .id(format!("menu-hidden-{}", ix))
                                                            .h(px(0.0))
                                                            .w(px(0.0))
                                                            .into_any_element();
                                                    }

                                                    let is_selected = ix == self.selected_index;
                                                    let is_active = self.active_id.as_deref() == Some(item.id.as_str());
                                                    let is_hovered = self.hovered_item == Some(ix);
                                                    let mouse_active = self.mouse_active;
                                                    let is_highlighted = is_active || (is_selected && !mouse_active) || is_hovered;
                                                    let action = item.action.clone();
                                                    let on_close = self.on_close.clone();

                                                    h_flex()
                                                        .id(format!("menu-item-{}", item.id))
                                                        .w_full()
                                                        .h(item_h)
                                                        .px(SPACE_MD)
                                                        .rounded(RADIUS_SM)
                                                        .items_center()
                                                        .justify_between()
                                                        .cursor_pointer()
                                                        .when((is_selected && !mouse_active) || is_active, |d| d.bg(sp.surface_hover))
                                                        .stateful_behavior(HoverBehavior { hover_bg: sp.surface_hover, ..Default::default() })
                                                        .on_hover(cx.listener(move |this, &hovered, _, cx| {
                                                            let next = if hovered { Some(ix) } else { None };
                                                            if this.hovered_item != next {
                                                                this.hovered_item = next;
                                                                cx.notify();
                                                            }
                                                        }))
                                                        .on_mouse_move(cx.listener(move |this, _, _, cx| {
                                                            if this.submenu_open.is_some() {
                                                                this.submenu_open = None;
                                                            }
                                                            if this.selected_index != ix || !this.mouse_active {
                                                                this.selected_index = ix;
                                                                this.mouse_active = true;
                                                                cx.notify();
                                                            }
                                                        }))
                                                        .on_click(move |_, window, cx| {
                                                            if let Some(ref on_close) = on_close {
                                                                on_close(window, cx);
                                                            }
                                                            action(window, cx);
                                                        })
                                                        .child(
                                                            h_flex()
                                                                .flex_1()
                                                                .min_w_0()
                                                                .items_center()
                                                                .gap(SPACE_MD)
                                                                .when_some(item.icon, |this, app_icon| {
                                                                    let icon_color = item.color_dot.map(|(c, _)| c);
                                                                    this.child(
                                                                        app_icon
                                                                            .size(px(15.0))
                                                                            .text_color(match icon_color {
                                                                                Some(c) => Hsla::from(rgb(c)),
                                                                                None => if is_highlighted { sp.text_primary } else { sp.text_muted },
                                                                            })
                                                                    )
                                                                })
                                                                .when_some(item.color_dot, |this, (color, hollow)| {
                                                                    if item.icon.is_some() {
                                                                        this
                                                                    } else {
                                                                        this.child(crate::color_dot::color_dot(color, hollow))
                                                                    }
                                                                })
                                                                .child(
                                                                    div()
                                                                        .id(format!("menu-item-label-{}", item.id))
                                                                        .flex_1()
                                                                        .min_w_0()
                                                                        .pr(SPACE_XS)
                                                                        .text_size(self.text_size)
                                                                        .text_color(sp.text_primary)
                                                                        .truncate()
                                                                        .when_some(
                                                                            self.item_tooltips
                                                                                .get(&SharedString::from(item.id.as_str()))
                                                                                .cloned(),
                                                                            |this, tip| {
                                                                                this.tooltip(move |_, cx| { let __tip = tip.clone(); cx.new(|_| Tooltip::new(__tip)).into() })
                                                                            },
                                                                        )
                                                                        .child(item.label.clone())
                                                                )
                                                        )
                                                        .when_some(item.shortcut.clone(), |this, keys| {
                                                            this.child(
                                                                div()
                                                                    .flex_none()
                                                                    .pl(SPACE_LG)
                                                                    .text_size(ui_text_xs(cx))
                                                                    .text_color(sp.text_muted)
                                                                    .child(keys)
                                                            )
                                                        })
                                                        .when_some(
                                                            self.item_indicators
                                                                .get(&SharedString::from(item.id.as_str()))
                                                                .cloned(),
                                                            |this, indicator| {
                                                                this.child(match indicator {
                                                                    OverlayMenuIndicator::Dot(color) => div()
                                                                        .w(SPACE_MD)
                                                                        .h(SPACE_MD)
                                                                        .rounded(RADIUS_STD)
                                                                        .bg(rgb(color))
                                                                        .into_any_element(),
                                                                    OverlayMenuIndicator::Icon(app_icon) => app_icon
                                                                        .size(ICON_STD)
                                                                        .text_color(sp.text_muted)
                                                                        .into_any_element(),
                                                                })
                                                            },
                                                        )
                                                        .when(!item.trailing_actions.is_empty(), |this| {
                                                            let trailing = item.trailing_actions.clone();
                                                            this.child(
                                                                h_flex()
                                                                    .gap(SPACE_XS)
                                                                    .items_center()
                                                                    .children(trailing.into_iter().map(|act| {
                                                                        let action_fn = act.action.clone();
                                                                        let tooltip_text = act.tooltip.clone();
                                                                        let act_id = act.id.clone();
                                                                        let close_size = ui_text_md(cx) + px(4.0);
                                                                        let close_visible = is_hovered || is_selected;
                                                                        div()
                                                                            .id(format!("menu-item-action-{}-{}", item.id, act_id))
                                                                            .flex_shrink_0()
                                                                            .w(close_size)
                                                                            .h(close_size)
                                                                            .rounded(RADIUS_SM)
                                                                            .flex()
                                                                            .items_center()
                                                                            .justify_center()
                                                                            .cursor_pointer()
                                                                            .text_color(sp.text_muted)
                                                                            .opacity(if close_visible { 1.0 } else { 0.0 })
                                                                            .hover(|s| s.bg(rgba(0xf14c4c99)))
                                                                            .when(close_visible, |el| el.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()))
                                                                            .when(close_visible, |el| {
                                                                                el.on_click(move |_, window, cx| {
                                                                                    cx.stop_propagation();
                                                                                    action_fn(window, cx);
                                                                                })
                                                                            })
                                                                            .when_some(tooltip_text, |el, tip| {
                                                                                if close_visible {
                                                                                    el.tooltip(move |_, cx| {
                                                                                        let __tip = tip.clone();
                                                                                        cx.new(|_| Tooltip::new(__tip)).into()
                                                                                    })
                                                                                } else {
                                                                                    el
                                                                                }
                                                                            })
                                                                            .child(
                                                                                act.icon
                                                                                    .size(ui_text_md(cx))
                                                                                    .text_color(sp.text_muted),
                                                                            )
                                                                    }))
                                                            )
                                                        })
                                                        .into_any_element()
                                                }
                                                OverlayMenuEntry::CheckItem(item) => {
                                                    let filtered = self.filtered_indices();
                                                    let is_visible = filtered.contains(&ix);
                                                    if !is_visible {
                                                        return div()
                                                            .id(format!("menu-hidden-{}", ix))
                                                            .h(px(0.0))
                                                            .w(px(0.0))
                                                            .into_any_element();
                                                    }

                                                    let is_selected = ix == self.selected_index;
                                                    let is_active = self.active_id.as_deref() == Some(item.id.as_str());
                                                    let is_hovered = self.hovered_item == Some(ix);
                                                    let mouse_active = self.mouse_active;
                                                    let is_highlighted = is_active || (is_selected && !mouse_active) || is_hovered;
                                                    let action = item.action.clone();
                                                    let on_close = self.on_close.clone();

                                                    h_flex()
                                                        .id(format!("menu-item-{}", item.id))
                                                        .w_full()
                                                        .h(item_h)
                                                        .px(SPACE_MD)
                                                        .rounded(RADIUS_SM)
                                                        .items_center()
                                                        .justify_between()
                                                        .cursor_pointer()
                                                        .when((is_selected && !mouse_active) || is_active, |d| d.bg(sp.surface_hover))
                                                        .stateful_behavior(HoverBehavior { hover_bg: sp.surface_hover, ..Default::default() })
                                                        .on_hover(cx.listener(move |this, &hovered, _, cx| {
                                                            let next = if hovered { Some(ix) } else { None };
                                                            if this.hovered_item != next {
                                                                this.hovered_item = next;
                                                                cx.notify();
                                                            }
                                                        }))
                                                        .on_mouse_move(cx.listener(move |this, _, _, cx| {
                                                            if this.selected_index != ix || !this.mouse_active {
                                                                this.selected_index = ix;
                                                                this.mouse_active = true;
                                                                cx.notify();
                                                            }
                                                        }))
                                                        .on_click(move |_, window, cx| {
                                                            if let Some(ref on_close) = on_close {
                                                                on_close(window, cx);
                                                            }
                                                            action(window, cx);
                                                        })
                                                        .child(
                                                            h_flex()
                                                                .flex_1()
                                                                .min_w_0()
                                                                .items_center()
                                                                .gap(SPACE_MD)
                                                                .when_some(item.icon, |this, app_icon| {
                                                                    this.child(
                                                                        app_icon
                                                                            .size(px(15.0))
                                                                            .text_color(if is_highlighted { sp.text_primary } else { sp.text_muted })
                                                                    )
                                                                })
                                                                .child(
                                                                    div()
                                                                        .id(format!("menu-item-label-{}", item.id))
                                                                        .flex_1()
                                                                        .min_w_0()
                                                                        .pr(SPACE_XS)
                                                                        .text_size(self.text_size)
                                                                        .text_color(sp.text_primary)
                                                                        .truncate()
                                                                        .when_some(
                                                                            self.item_tooltips
                                                                                .get(&SharedString::from(item.id.as_str()))
                                                                                .cloned(),
                                                                            |this, tip| {
                                                                                this.tooltip(move |_, cx| { let __tip = tip.clone(); cx.new(|_| Tooltip::new(__tip)).into() })
                                                                            },
                                                                        )
                                                                        .child(item.label.clone())
                                                                )
                                                        )
                                                        .child(
                                                            if item.checked {
                                                                AppIcon::Check
                                                                    .size(px(15.0))
                                                                    .text_color(sp.surface_accent)
                                                                    .into_any_element()
                                                            } else {
                                                                div()
                                                                    .size(px(15.0))
                                                                    .into_any_element()
                                                            },
                                                        )
                                                        .into_any_element()
                                                }
                                            }
                                        }))
                                )
                        )
                )
        )
        .child(
            if let Some(ref open_id) = self.submenu_open {
                let item_bounds = self.submenu_bounds.get(open_id).cloned();
                let menu_bounds = self.main_menu_bounds.unwrap_or_else(|| self.trigger_bounds);
                let sub_pos = if let Some(item_bounds) = item_bounds {
                    point(
                        menu_bounds.origin.x + menu_bounds.size.width + px(2.0),
                        item_bounds.origin.y - px(4.0),
                    )
                } else {
                    point(px(0.0), px(0.0))
                };
                let entries = self.items.iter().find_map(|entry| {
                    if let OverlayMenuEntry::Submenu(sub) = entry {
                        if sub.id == *open_id {
                            return Some(sub.entries.clone());
                        }
                    }
                    None
                });
                if let Some(entries) = entries {
                    deferred(
                        anchored()
                            .position(sub_pos)
                            .anchor(gpui::Anchor::TopLeft)
                            .snap_to_window()
                            .child(Self::render_submenu_panel(
                                &entries, &self.on_close.clone(), self.text_size, &t, &sp, cx,
                            )),
                    )
                    .into_any_element()
                } else {
                    div().invisible().size_0().into_any_element()
                }
            } else {
                div().invisible().size_0().into_any_element()
            },
        )
    }
}
