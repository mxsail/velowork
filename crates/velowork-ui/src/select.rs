//! Reusable `Select` / Dropdown component for Velowork UI.
//!
//! # Architecture & Responsibilities vs `QuickPicker`:
//! - **`Select`** ([`Select`]): For **In-Form Dropdown Controls** (e.g. settings form fields, baud rate selectors, proxy mode)
//!   which anchor below/above a specific trigger element in a form or dialog using `anchored()`.
//! - **`QuickPicker`** ([`QuickPickerState`](crate::quick_picker::QuickPickerState)): For **Global Modal Command Palettes & Quick Open Dialogs**
//!   (e.g., Project Switcher `Ctrl+P`, Theme Selector `Ctrl+K Ctrl+T`, Shell Selector, Command Palette `Ctrl+Shift+P`).
//!
//! Provides a feature-complete, accessible dropdown selection control with:
//! - Single & grouped option lists (`SelectOption`, `SelectGroup`)
//! - Placement direction control (`SelectPlacement::Auto`, `Above`, `Below`)
//! - Optional search/filter input (`searchable`)
//! - One-click clearing (`cleanable`)
//! - Custom item rendering callback
//! - Self-registering `OverlayRegistry` integration for click-outside dismissal
//! - Keyboard navigation (Up/Down arrows, Enter to confirm, Escape to cancel)
//! - Focus handle & accessibility state management

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use gpui::*;
use gpui::prelude::*;
use velowork_i18n::i18n;

use crate::Cancel;
use crate::icon::AppIcon;
use crate::overlay_registry::{ClosePolicy, OverlayInfo, OverlayRegistry};
use crate::input::{focus_ring_shadows, Input, InputChangedEvent, InputState};
use crate::scrollable::{Scrollbar, ScrollbarShow};
use crate::theme::{surface_bg, theme, ThemeColors};
use crate::tokens::{
    ui_icon_sm, ui_icon_std_ts, ui_text_md, ui_text_sm, ui_text_xs, DIALOG_LG, DIALOG_SM,
    POPOVER_LIST_HEADER_H, POPOVER_LIST_MAX_H, POPOVER_LIST_MIN_H,
    RADIUS_MD, RADIUS_SM, SELECT_WIDTH_MD, SPACE_2XL, SPACE_2XS,
    SPACE_MD, SPACE_SM, SPACE_XL, SPACE_XS,
};
use crate::design::appearance::{ControlSize, control_height_for_size};

/// Placement direction of the [`Select`] popover.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectPlacement {
    /// Automatically determine placement (Above if in lower window region, Below otherwise).
    #[default]
    Auto,
    /// Open popover above the trigger.
    Above,
    /// Open popover below the trigger.
    Below,
}

/// 下拉选项面板的宽度模式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectWidthMode {
    /// 与展示框宽度相同（超长选项自动截断，默认）
    #[default]
    MatchTrigger,
    /// 根据内容自适应宽度（最小宽度为展示框宽度，超长时右侧扩展至窗口边界，超长文字截断）
    ContentAdaptive,
}

/// A single option item inside a [`Select`].
#[derive(Clone)]
pub struct SelectOption<T: Clone + PartialEq + 'static> {
    pub value: T,
    pub label: SharedString,
    pub description: Option<SharedString>,
    pub icon: Option<AppIcon>,
    pub disabled: bool,
}

impl<T: Clone + PartialEq + 'static> SelectOption<T> {
    pub fn new(value: T, label: impl Into<SharedString>) -> Self {
        Self {
            value,
            label: label.into(),
            description: None,
            icon: None,
            disabled: false,
        }
    }

    pub fn description(mut self, desc: impl Into<SharedString>) -> Self {
        self.description = Some(desc.into());
        self
    }

    pub fn icon(mut self, icon: AppIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

/// A group of related options with an optional title.
#[derive(Clone)]
pub struct SelectGroup<T: Clone + PartialEq + 'static> {
    pub title: Option<SharedString>,
    pub options: Vec<SelectOption<T>>,
}

impl<T: Clone + PartialEq + 'static> SelectGroup<T> {
    pub fn new(options: Vec<SelectOption<T>>) -> Self {
        Self {
            title: None,
            options,
        }
    }

    pub fn with_title(title: impl Into<SharedString>, options: Vec<SelectOption<T>>) -> Self {
        Self {
            title: Some(title.into()),
            options,
        }
    }

    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }
}

/// Events emitted by [`SelectState`].
#[derive(Clone)]
pub enum SelectEvent<T: Clone + PartialEq + 'static> {
    /// Emitted when the selection changes. `None` if cleared.
    Change(Option<T>),
}

type CustomRenderOption<T> = Rc<dyn Fn(&SelectOption<T>, bool, &ThemeColors, &App) -> AnyElement>;

#[derive(Clone)]
enum SelectRowItem<T: Clone + PartialEq + 'static> {
    Header(SharedString),
    Option {
        flat_idx: usize,
        option: SelectOption<T>,
    },
}

/// State of the [`Select`] dropdown component.
pub struct SelectState<T: Clone + PartialEq + 'static> {
    groups: Vec<SelectGroup<T>>,
    selected_value: Option<T>,
    placeholder: SharedString,
    search_placeholder: Option<SharedString>,
    search_text: String,
    searchable: bool,
    cleanable: bool,
    disabled: bool,
    open: bool,
    placement: SelectPlacement,
    width_mode: SelectWidthMode,
    virtual_scroll: bool,
    show_scrollbar: bool,
    uniform_scroll_handle: UniformListScrollHandle,
    scroll_handle: ScrollHandle,
    hovered_flat_index: Option<usize>,
    focus_handle: FocusHandle,
    search_input: Option<Entity<InputState>>,
    overlay_registry: Option<Entity<OverlayRegistry>>,
    overlay_id: SharedString,
    trigger_bounds: Bounds<Pixels>,
    custom_render_option: Option<CustomRenderOption<T>>,
    size: ControlSize,
    ghost: bool,
    text_size: Option<Pixels>,
}

impl<T: Clone + PartialEq + 'static> EventEmitter<SelectEvent<T>> for SelectState<T> {}

impl<T: Clone + PartialEq + 'static> SelectState<T> {
    /// Create a new [`SelectState`].
    pub fn new(cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let overlay_id: SharedString = format!("select-overlay-{}", cx.entity_id()).into();
        let overlay_registry = OverlayRegistry::global(cx);

        Self {
            groups: Vec::new(),
            selected_value: None,
            placeholder: i18n!(cx, "common.select_placeholder").into(),
            search_placeholder: None,
            search_text: String::new(),
            searchable: false,
            cleanable: false,
            disabled: false,
            open: false,
            placement: SelectPlacement::Auto,
            width_mode: SelectWidthMode::default(),
            virtual_scroll: false,
            show_scrollbar: true,
            uniform_scroll_handle: UniformListScrollHandle::new(),
            scroll_handle: ScrollHandle::new(),
            hovered_flat_index: None,
            focus_handle,
            search_input: None,
            overlay_registry,
            overlay_id,
            trigger_bounds: Bounds::default(),
            custom_render_option: None,
            size: ControlSize::Default,
            ghost: false,
            text_size: None,
        }
    }

    /// Set options as a single group.
    pub fn options(mut self, options: Vec<SelectOption<T>>) -> Self {
        self.groups = vec![SelectGroup::new(options)];
        self
    }

    /// Update options dynamically.
    pub fn set_options(&mut self, options: Vec<SelectOption<T>>, cx: &mut Context<Self>) {
        self.groups = vec![SelectGroup::new(options)];
        cx.notify();
    }

    /// Set grouped options.
    pub fn groups(mut self, groups: Vec<SelectGroup<T>>) -> Self {
        self.groups = groups;
        self
    }

    /// Set initial or current selected value.
    pub fn selected(mut self, value: Option<T>) -> Self {
        self.selected_value = value;
        self
    }

    /// Set placeholder text when no option is selected.
    pub fn placeholder(mut self, text: impl Into<SharedString>) -> Self {
        self.placeholder = text.into();
        self
    }

    /// Set placeholder text for search filter input.
    pub fn search_placeholder(mut self, text: impl Into<SharedString>) -> Self {
        self.search_placeholder = Some(text.into());
        self
    }

    /// Enable or disable inline search filter.
    pub fn searchable(mut self, searchable: bool) -> Self {
        self.searchable = searchable;
        self
    }

    /// Enable or disable clear button when an option is selected.
    pub fn cleanable(mut self, cleanable: bool) -> Self {
        self.cleanable = cleanable;
        self
    }

    /// Set disabled state.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Set popover placement direction (`Auto`, `Above`, `Below`).
    pub fn placement(mut self, placement: SelectPlacement) -> Self {
        self.placement = placement;
        self
    }

    /// Update placement direction dynamically.
    pub fn set_placement(&mut self, placement: SelectPlacement, cx: &mut Context<Self>) {
        self.placement = placement;
        cx.notify();
    }

    /// 设置下拉面板宽度模式（`MatchTrigger` 与展示框等宽，`ContentAdaptive` 自适应内容宽度）
    pub fn width_mode(mut self, mode: SelectWidthMode) -> Self {
        self.width_mode = mode;
        self
    }

    /// Set control size Tier (`Default`, `Compact`, `Large`).
    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }

    /// Update control size Tier dynamically.
    pub fn set_size(&mut self, size: ControlSize, cx: &mut Context<Self>) {
        self.size = size;
        cx.notify();
    }

    /// Set ghost mode (transparent trigger background, compact height, minimal borders).
    pub fn ghost(mut self, ghost: bool) -> Self {
        self.ghost = ghost;
        self
    }

    /// Update ghost mode dynamically.
    pub fn set_ghost(&mut self, ghost: bool, cx: &mut Context<Self>) {
        self.ghost = ghost;
        cx.notify();
    }

    /// Set custom text font size for trigger.
    pub fn text_size(mut self, size: impl Into<Pixels>) -> Self {
        self.text_size = Some(size.into());
        self
    }

    /// Update custom text font size dynamically.
    pub fn set_text_size(&mut self, size: Option<Pixels>, cx: &mut Context<Self>) {
        if self.text_size != size {
            self.text_size = size;
            cx.notify();
        }
    }

    /// 动态设置下拉面板宽度模式
    pub fn set_width_mode(&mut self, mode: SelectWidthMode, cx: &mut Context<Self>) {
        self.width_mode = mode;
        cx.notify();
    }

    /// 开启或关闭虚拟滚动（针对大量选项列表，如几十/几百个系统字体时推荐开启）
    pub fn virtual_scroll(mut self, enable: bool) -> Self {
        self.virtual_scroll = enable;
        self
    }

    /// 动态设置是否开启虚拟滚动
    pub fn set_virtual_scroll(&mut self, enable: bool, cx: &mut Context<Self>) {
        self.virtual_scroll = enable;
        cx.notify();
    }

    /// 开启或关闭滚动条展示（默认开启）
    pub fn show_scrollbar(mut self, show: bool) -> Self {
        self.show_scrollbar = show;
        self
    }

    /// 动态设置是否展示滚动条
    pub fn set_show_scrollbar(&mut self, show: bool, cx: &mut Context<Self>) {
        self.show_scrollbar = show;
        cx.notify();
    }

    /// Inject window-level `OverlayRegistry` for automatic click-outside dismissal.
    pub fn set_overlay_registry(&mut self, registry: Entity<OverlayRegistry>) {
        self.overlay_registry = Some(registry);
    }

    /// Set custom option row renderer.
    pub fn render_option(
        mut self,
        render: impl Fn(&SelectOption<T>, bool, &ThemeColors, &App) -> AnyElement + 'static,
    ) -> Self {
        self.custom_render_option = Some(Rc::new(render));
        self
    }

    /// Get current selected value.
    pub fn selected_value(&self) -> Option<&T> {
        self.selected_value.as_ref()
    }

    /// Set selected value programmatically.
    pub fn set_selected_value(&mut self, value: Option<T>, cx: &mut Context<Self>) {
        if self.selected_value != value {
            self.selected_value = value.clone();
            cx.emit(SelectEvent::Change(value));
            cx.notify();
        }
    }

    /// Get open state.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Toggle open state.
    fn ensure_search_input(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        if let Some(ref input) = self.search_input {
            return input.clone();
        }
        let ph = if self.searchable {
            self.selected_option()
                .map(|o| o.label.to_string())
                .unwrap_or_else(|| self.placeholder.to_string())
        } else {
            i18n!(cx, "common.search_placeholder").to_string()
        };
        let input = cx.new(|cx| {
            InputState::new(cx).placeholder(ph)
        });
        cx.subscribe(&input, |this, _, _event: &InputChangedEvent, cx| {
            let val = this
                .search_input
                .as_ref()
                .map(|i| i.read(cx).text().to_string())
                .unwrap_or_default();
            this.search_text = val;
            this.hovered_flat_index = None;
            if this.virtual_scroll {
                this.uniform_scroll_handle.scroll_to_item(0, ScrollStrategy::Top);
            } else {
                this.scroll_handle.scroll_to_item(0);
            }
            cx.notify();
        })
        .detach();
        self.search_input = Some(input.clone());
        input
    }

    /// Open or close the dropdown popover.
    pub fn set_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }

        if self.open != open {
            self.open = open;
            if self.open {
                if self.overlay_registry.is_none() {
                    self.overlay_registry = OverlayRegistry::global(cx);
                }
                self.search_text.clear();
                let input = self.ensure_search_input(window, cx);
                let ph = if self.searchable {
                    self.selected_option()
                        .map(|o| o.label.to_string())
                        .unwrap_or_else(|| self.placeholder.to_string())
                } else {
                    i18n!(cx, "common.search_placeholder").to_string()
                };
                input.update(cx, |s, cx| {
                    s.set_value("", cx);
                    s.set_placeholder(ph);
                });
                self.hovered_flat_index = None;
                self.register_overlay(window, cx);

                // Close all other same-tier (z_index <= 100) overlays to enforce
                // mutual exclusivity among Select dropdowns.
                if let Some(registry) = self.get_overlay_registry(cx) {
                    let overlay_id = self.overlay_id.clone();
                    registry.update(cx, |r, cx| {
                        r.close_others(&overlay_id, 100, window, cx);
                    });
                }

                if self.virtual_scroll {
                    let flat_opts = self.flattened_visible_options();
                    if let Some(selected_val) = &self.selected_value
                        && let Some(pos) = flat_opts.iter().position(|o| &o.value == selected_val)
                    {
                        self.hovered_flat_index = Some(pos);
                        self.uniform_scroll_handle.scroll_to_item(pos, ScrollStrategy::Center);
                    }
                } else {
                    let flat_opts = self.flattened_visible_options();
                    if let Some(selected_val) = &self.selected_value
                        && let Some(pos) = flat_opts.iter().position(|o| &o.value == selected_val)
                    {
                        self.hovered_flat_index = Some(pos);
                        let child_idx = self.child_index_for_flat_index(pos);
                        self.scroll_handle.scroll_to_item(child_idx);
                    } else {
                        self.scroll_handle.set_offset(Point::default());
                    }
                }
                if self.searchable {
                    let search_input = input.clone();
                    window.defer(cx, move |window, cx| {
                        search_input.update(cx, |s, cx| s.focus(window, cx));
                    });
                } else {
                    // Non-searchable: focus the trigger immediately so this
                    // Select becomes the active one and the previous one loses
                    // its active highlight.
                    self.focus_handle.focus(window, cx);
                }
            } else {
                self.unregister_overlay(cx);
                self.focus_handle.focus(window, cx);
            }
            cx.notify();
        }
    }

    /// Focus this select trigger.
    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_handle.focus(window, cx);
    }

    /// Access the focus handle for this select trigger.
    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }

    fn get_overlay_registry(&self, cx: &App) -> Option<Entity<OverlayRegistry>> {
        self.overlay_registry
            .clone()
            .or_else(|| OverlayRegistry::global(cx))
    }

    fn register_overlay(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(registry) = self.get_overlay_registry(cx) {
            let entity_weak = cx.entity().downgrade();
            let info = OverlayInfo {
                id: self.overlay_id.clone(),
                bounds: self.trigger_bounds,
                secondary_bounds: None,
                close_policy: ClosePolicy::ClickOutside,
                z_index: 100,
            };

            let close_fn = Arc::new(move |_window: &mut Window, cx: &mut App| {
                if let Some(entity) = entity_weak.upgrade() {
                    entity.update(cx, |this, cx| {
                        this.open = false;
                        cx.notify();
                    });
                }
            });

            registry.update(cx, |r, _| {
                r.register(info, close_fn);
            });
        }
    }

    fn unregister_overlay(&mut self, cx: &mut Context<Self>) {
        if let Some(registry) = self.get_overlay_registry(cx) {
            let id = self.overlay_id.clone();
            registry.update(cx, |r, _| {
                r.unregister(&id);
            });
        }
    }

    /// Get all visible options after search filtering.
    fn visible_filtered_groups(&self) -> Vec<SelectGroup<T>> {
        let query = self.search_text.trim().to_lowercase();
        if query.is_empty() {
            return self.groups.clone();
        }

        self.groups
            .iter()
            .filter_map(|group| {
                let filtered: Vec<SelectOption<T>> = group
                    .options
                    .iter()
                    .filter(|opt| {
                        opt.label.to_lowercase().contains(&query)
                            || opt
                                .description
                                .as_ref()
                                .is_some_and(|d| d.to_lowercase().contains(&query))
                    })
                    .cloned()
                    .collect();

                if filtered.is_empty() {
                    None
                } else {
                    Some(SelectGroup {
                        title: group.title.clone(),
                        options: filtered,
                    })
                }
            })
            .collect()
    }

    /// Flatten visible options for index-based keyboard navigation.
    fn flattened_visible_options(&self) -> Vec<SelectOption<T>> {
        self.visible_filtered_groups()
            .into_iter()
            .flat_map(|g| g.options)
            .collect()
    }

    /// Map flattened option index to direct child index in the non-virtual scroll area,
    /// accounting for group header elements.
    fn child_index_for_flat_index(&self, flat_index: usize) -> usize {
        let mut child_count = 0;
        let mut opt_count = 0;
        for group in self.visible_filtered_groups() {
            if group.title.is_some() {
                child_count += 1;
            }
            for _ in &group.options {
                if opt_count == flat_index {
                    return child_count;
                }
                opt_count += 1;
                child_count += 1;
            }
        }
        flat_index
    }

    /// Find current selected option label/icon if any.
    fn selected_option(&self) -> Option<SelectOption<T>> {
        let selected_val = self.selected_value.as_ref()?;
        for g in &self.groups {
            for opt in &g.options {
                if &opt.value == selected_val {
                    return Some(opt.clone());
                }
            }
        }
        None
    }

    fn handle_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let flat_opts = self.flattened_visible_options();
        if flat_opts.is_empty() {
            return;
        }

        match event.keystroke.key.as_str() {
            "down" => {
                if !self.open {
                    self.set_open(true, window, cx);
                } else {
                    let next = match self.hovered_flat_index {
                        Some(idx) => (idx + 1) % flat_opts.len(),
                        None => 0,
                    };
                    self.hovered_flat_index = Some(next);
                    if self.virtual_scroll {
                        self.uniform_scroll_handle.scroll_to_item(next, ScrollStrategy::Bottom);
                    } else {
                        let child_idx = self.child_index_for_flat_index(next);
                        self.scroll_handle.scroll_to_item(child_idx);
                    }
                    cx.notify();
                }
            }
            "up" if self.open => {
                let prev = match self.hovered_flat_index {
                    Some(idx) => if idx == 0 { flat_opts.len() - 1 } else { idx - 1 },
                    None => flat_opts.len() - 1,
                };
                self.hovered_flat_index = Some(prev);
                if self.virtual_scroll {
                    self.uniform_scroll_handle.scroll_to_item(prev, ScrollStrategy::Top);
                } else {
                    let child_idx = self.child_index_for_flat_index(prev);
                    self.scroll_handle.scroll_to_item(child_idx);
                }
                cx.notify();
            }
            "enter" | "\n" | "space" | " " => {
                if self.open {
                    if let Some(idx) = self.hovered_flat_index
                        && let Some(opt) = flat_opts.get(idx)
                        && !opt.disabled
                    {
                        self.set_selected_value(Some(opt.value.clone()), cx);
                        self.set_open(false, window, cx);
                    }
                } else {
                    self.set_open(true, window, cx);
                }
            }
            "escape" if self.open => {
                cx.stop_propagation();
                self.set_open(false, window, cx);
            }
            _ => {}
        }
    }
}

impl<T: Clone + PartialEq + 'static> Render for SelectState<T> {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let is_focused = self.focus_handle.is_focused(window);
        let selected_opt = self.selected_option();
        let has_selection = selected_opt.is_some();
        let show_clear = self.cleanable && has_selection && !self.disabled;

        let p = crate::design::semantic::SemanticPalette::from_context(cx);
        let is_active = self.open || is_focused;
        let border_color = if self.ghost {
            if is_active {
                p.border_subtle
            } else {
                gpui::transparent_black()
            }
        } else if is_active {
            p.border_active
        } else {
            p.border_subtle
        };

        let bg_color = if self.ghost {
            if is_active {
                p.surface_hover
            } else {
                gpui::transparent_black()
            }
        } else if is_active {
            p.surface_hover
        } else {
            p.surface_card
        };

        let text_font_size = self.text_size.unwrap_or_else(|| {
            if self.ghost {
                ui_text_sm(cx)
            } else {
                ui_text_md(cx)
            }
        });
        let trigger_h = if self.ghost {
            if self.text_size.is_some() {
                control_height_for_size(self.size, cx)
            } else {
                px(22.0)
            }
        } else {
            control_height_for_size(self.size, cx)
        };
        let trigger_px = if self.ghost {
            px(6.0)
        } else {
            SPACE_MD
        };
        let chevron_size = if self.ghost {
            if self.text_size.is_some() {
                ui_icon_sm(cx)
            } else {
                px(11.0)
            }
        } else {
            ui_icon_std_ts(cx)
        };

        // Store bounds via on_prepaint for OverlayRegistry alignment
        let state_entity = cx.entity();

        // Left Label / Icon / Placeholder
        // When searchable and open, the trigger morphs into an inline search
        // box (its placeholder already shows the current selection). Clicks on
        // the input must not bubble to the trigger's toggle handler.
        let left_content: AnyElement = if self.open && self.searchable {
            let input = self.ensure_search_input(window, cx);
            div()
                .flex()
                .flex_1()
                .items_center()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(Input::new(&input).appearance(false).text_size(text_font_size))
                .into_any_element()
        } else if let Some(opt) = selected_opt {
            let icon_el = opt.icon.map(|ic| ic.size(ui_icon_sm(cx)).text_color(rgb(t.text_primary)));
            div()
                .flex()
                .items_center()
                .gap(SPACE_SM)
                .truncate()
                .children(icon_el)
                .child(div().text_size(text_font_size).text_color(rgb(t.text_primary)).child(opt.label))
                .into_any_element()
        } else {
            div()
                .flex()
                .items_center()
                .gap(SPACE_SM)
                .text_size(text_font_size)
                .text_color(rgb(t.text_muted))
                .child(self.placeholder.clone())
                .into_any_element()
        };

        // Right Action Icon (Chevron / Clear)
        let right_icon = if show_clear {
            div()
                .id("select-clear-btn")
                .cursor_pointer()
                .p(px(2.0))
                .rounded(RADIUS_MD)
                .hover(|s| s.bg(p.surface_hover))
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                    this.set_selected_value(None, cx);
                }))
                .child(AppIcon::Close.size(chevron_size).text_color(rgb(t.text_muted)))
                .into_any_element()
        } else {
            let chevron_icon = if self.open { AppIcon::ChevronUp } else { AppIcon::ChevronDown };
            div().child(chevron_icon.size(chevron_size).text_color(rgb(t.text_muted))).into_any_element()
        };

        let bounds_listener = move |bounds: Bounds<Pixels>, _window: &mut Window, cx: &mut App| {
            state_entity.update(cx, |this, _| {
                this.trigger_bounds = bounds;
            });
        };

        // Trigger Container
        let trigger = div()
            .id("select-trigger")
            .track_focus(&self.focus_handle)
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_between()
            .h(trigger_h)
            .px(trigger_px)
            .bg(bg_color)
            .rounded(RADIUS_MD)
            .border_1()
            .border_color(border_color)
            .when(is_active && !self.disabled && !self.ghost, |d| d.shadow(focus_ring_shadows(&t)))
            .when(!self.disabled && !is_active, |d| {
                if self.ghost {
                    d.hover(|s| s.bg(p.surface_hover))
                } else {
                    d.hover(|s| s.border_color(p.surface_accent.opacity(0.6)).bg(p.surface_hover))
                }
            })
            .when(self.disabled, |d| d.opacity(0.5).cursor_not_allowed())
            .on_action(cx.listener(|this, _: &Cancel, window, cx| {
                if this.open {
                    this.set_open(false, window, cx);
                } else {
                    cx.propagate();
                }
            }))
            .on_key_down(cx.listener(|this, e, window, cx| {
                this.handle_key_down(e, window, cx);
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| {
                cx.stop_propagation();
                this.focus_handle.focus(window, cx);
                let next_open = !this.open;
                this.set_open(next_open, window, cx);
            }))
            .child(left_content)
            .child(right_icon)
            .child(canvas(bounds_listener, |_, _, _, _| {}).absolute().inset_0());

        // --- Render Dropdown Overlay Popover when open ---
        let filtered_groups = self.visible_filtered_groups();
        let mut flat_counter = 0usize;

        let popover_overlay = if self.open {
            let custom_render = self.custom_render_option.clone();
            let current_selected_val = self.selected_value.clone();
            let hovered_flat_idx = self.hovered_flat_index;

            let viewport_size = window.viewport_size();
            let window_w = viewport_size.width;
            let window_h = viewport_size.height;
            let margin = SPACE_MD;

            let trigger_x = self.trigger_bounds.origin.x;
            let trigger_y = self.trigger_bounds.origin.y;
            let trigger_w = self.trigger_bounds.size.width;
            let trigger_h = self.trigger_bounds.size.height;

            let trigger_min_w = if trigger_w > px(0.0) { trigger_w } else { SELECT_WIDTH_MD };

            let (popover_style_w, popover_style_max_w) = match self.width_mode {
                SelectWidthMode::MatchTrigger => (Some(trigger_min_w), Some(trigger_min_w)),
                SelectWidthMode::ContentAdaptive => {
                    let max_w = if window_w > px(0.0) {
                        (window_w - trigger_x - margin).max(trigger_min_w)
                    } else {
                        DIALOG_LG
                    };
                    (None, Some(max_w))
                }
            };

            let space_below = if window_h > px(0.0) {
                window_h - (trigger_y + trigger_h + SPACE_XS) - margin
            } else {
                DIALOG_SM
            };

            let space_above = trigger_y - SPACE_XS - margin;

            let effective_above = match self.placement {
                SelectPlacement::Above => true,
                SelectPlacement::Below => false,
                SelectPlacement::Auto => space_below < SPACE_2XL && space_above > space_below,
            };

            let max_popover_h = if effective_above {
                space_above.clamp(px(100.0), POPOVER_LIST_MAX_H)
            } else {
                space_below.clamp(px(100.0), POPOVER_LIST_MAX_H)
            };

            let opt_h = crate::menu::menu_item_height(cx);

            let popover_list = if filtered_groups.is_empty() {
                let empty_tip = i18n!(cx, "common.state.no_results");
                div()
                    .id("select-popover-list")
                    .flex()
                    .flex_col()
                    .gap(SPACE_2XS)
                    .p(SPACE_XS)
                    .max_h(max_popover_h)
                    .child(
                        div()
                            .px(SPACE_MD)
                            .py(SPACE_SM)
                            .text_size(ui_text_sm(cx))
                            .text_color(rgb(t.text_muted))
                            .child(empty_tip),
                    )
            } else if self.virtual_scroll {
                let mut row_items = Vec::new();
                let mut flat_c = 0usize;
                for group in filtered_groups {
                    if let Some(group_title) = group.title {
                        row_items.push(SelectRowItem::Header(group_title));
                    }
                    for opt in group.options {
                        row_items.push(SelectRowItem::Option {
                            flat_idx: flat_c,
                            option: opt,
                        });
                        flat_c += 1;
                    }
                }

                let item_count = row_items.len();
                let row_items = Rc::new(row_items);
                let state_entity = cx.entity();
                let uniform_handle = self.uniform_scroll_handle.clone();
                let custom_render = custom_render.clone();
                let current_selected_val = current_selected_val.clone();
                let row_items_clone = row_items.clone();

                let list_h = ((opt_h + SPACE_2XS) * item_count as f32 + SPACE_MD).clamp(POPOVER_LIST_MIN_H, max_popover_h);

                let ulist = uniform_list(
                    ElementId::Name(format!("select-ulist-{}", self.overlay_id).into()),
                    item_count,
                    move |range, _window, cx| {
                        let t = theme(cx);
                        let p = crate::design::semantic::SemanticPalette::from_theme(&t);
                        let mut elements = Vec::with_capacity(range.len());
                        for i in range {
                            if let Some(item) = row_items_clone.get(i) {
                                match item {
                                    SelectRowItem::Header(title) => {
                                        elements.push(
                                            div()
                                                .h(POPOVER_LIST_HEADER_H)
                                                .flex()
                                                .items_center()
                                                .px(SPACE_MD)
                                                .text_size(ui_text_xs(cx))
                                                .font_weight(FontWeight::BOLD)
                                                .text_color(rgb(t.text_muted))
                                                .child(title.clone())
                                                .into_any_element(),
                                        );
                                    }
                                    SelectRowItem::Option { flat_idx, option } => {
                                        let item_idx = *flat_idx;
                                        let is_selected = current_selected_val.as_ref() == Some(&option.value);
                                        let is_hovered = hovered_flat_idx == Some(item_idx);
                                        let is_disabled = option.disabled;
                                        let opt = option.clone();

                                        let row_el = if let Some(ref custom_fn) = custom_render {
                                            custom_fn(&opt, is_selected, &t, cx)
                                        } else {
                                            let item_bg = if is_selected {
                                                p.surface_selection
                                            } else if is_hovered {
                                                p.surface_hover
                                            } else {
                                                transparent_black()
                                            };

                                            let text_color = if is_disabled {
                                                rgb(t.text_muted)
                                            } else if is_selected {
                                                rgb(t.text_primary)
                                            } else {
                                                rgb(t.text_secondary)
                                            };

                                            let icon_el = opt.icon.map(|ic| ic.size(ui_icon_sm(cx)).text_color(text_color));

                                            let mut row = div()
                                                .id(ElementId::Name(format!("select-opt-{}", item_idx).into()))
                                                .w_full()
                                                .h(opt_h)
                                                .flex()
                                                .items_center()
                                                .justify_between()
                                                .gap(SPACE_MD)
                                                .pl(SPACE_MD)
                                                .pr(SPACE_XL)
                                                .rounded(RADIUS_SM)
                                                .bg(item_bg)
                                                .text_color(text_color)
                                                .text_size(ui_text_md(cx));

                                            if !is_disabled {
                                                row = row
                                                    .cursor_pointer()
                                                    .hover(|s| s.bg(p.surface_hover));
                                            }

                                            let row_left = div()
                                                .flex()
                                                .items_center()
                                                .gap(SPACE_SM)
                                                .min_w(px(0.0))
                                                .flex_1()
                                                .children(icon_el);
                                            let row_left = if let Some(ref desc) = opt.description {
                                                row_left.child(
                                                    div()
                                                        .flex()
                                                        .flex_col()
                                                        .min_w(px(0.0))
                                                        .child(div().whitespace_nowrap().child(opt.label.clone()))
                                                        .child(div().truncate().text_size(ui_text_xs(cx)).text_color(rgb(t.text_muted)).child(desc.clone())),
                                                )
                                            } else {
                                                row_left.child(div().whitespace_nowrap().child(opt.label.clone()))
                                            };

                                            let check_el = if is_selected {
                                                Some(AppIcon::Check.size(ui_icon_sm(cx)).text_color(rgb(t.text_primary)))
                                            } else {
                                                None
                                            };

                                            row.child(row_left).children(check_el).into_any_element()
                                        };

                                        let val_clone = opt.value.clone();
                                        let state_entity_clone = state_entity.clone();
                                        let row_wrapper = div()
                                            .w_full()
                                            .child(row_el)
                                            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                                if !is_disabled {
                                                    state_entity_clone.update(cx, |this, cx| {
                                                        this.set_selected_value(Some(val_clone.clone()), cx);
                                                        this.set_open(false, window, cx);
                                                    });
                                                }
                                            });

                                        elements.push(row_wrapper.into_any_element());
                                    }
                                }
                            }
                        }
                        elements
                    },
                )
                .track_scroll(&uniform_handle)
                .size_full();

                let mut container = div()
                    .id("select-popover-list")
                    .relative()
                    .flex()
                    .flex_col()
                    .w_full()
                    .h(list_h)
                    .p(SPACE_XS)
                    .child(ulist);

                if self.show_scrollbar {
                    container = container.child(
                        div()
                            .absolute()
                            .top(SPACE_XS)
                            .bottom(SPACE_XS)
                            .left(SPACE_XS)
                            .right(SPACE_XS)
                            .child(
                                Scrollbar::vertical(&self.uniform_scroll_handle)
                                    .scrollbar_show(ScrollbarShow::Hover),
                            ),
                    );
                }

                container
            } else {
                let mut scroll_area = div()
                    .id("select-popover-list-scroll")
                    .relative()
                    .flex()
                    .flex_col()
                    .gap(SPACE_2XS)
                    .p(SPACE_XS)
                    .w_full()
                    .max_h(max_popover_h)
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll_handle);

                for group in filtered_groups {
                    if let Some(group_title) = group.title {
                        scroll_area = scroll_area.child(
                            div()
                                .px(SPACE_MD)
                                .pt(SPACE_SM)
                                .pb(SPACE_XS)
                                .text_size(ui_text_xs(cx))
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(t.text_muted))
                                .child(group_title),
                        );
                    }

                    for opt in group.options {
                        let item_idx = flat_counter;
                        flat_counter += 1;

                        let is_selected = current_selected_val.as_ref() == Some(&opt.value);
                        let is_hovered = hovered_flat_idx == Some(item_idx);
                        let is_disabled = opt.disabled;

                        let row_el = if let Some(ref custom_fn) = custom_render {
                            custom_fn(&opt, is_selected, &t, cx)
                        } else {
                            let item_bg = if is_selected {
                                p.surface_selection
                            } else if is_hovered {
                                p.surface_hover
                            } else {
                                transparent_black()
                            };

                            let text_color = if is_disabled {
                                rgb(t.text_muted)
                            } else if is_selected {
                                rgb(t.text_primary)
                            } else {
                                rgb(t.text_secondary)
                            };

                            let icon_el = opt.icon.map(|ic| ic.size(ui_icon_sm(cx)).text_color(text_color));

                            let has_desc = opt.description.is_some();
                            let mut row = div()
                                .id(ElementId::Name(format!("select-opt-{}", item_idx).into()))
                                .w_full()
                                .when(!has_desc, |d| d.h(opt_h))
                                .when(has_desc, |d| d.min_h(opt_h).py(SPACE_SM))
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap(SPACE_MD)
                                .pl(SPACE_MD)
                                .pr(SPACE_XL)
                                .rounded(RADIUS_SM)
                                .bg(item_bg)
                                .text_color(text_color)
                                .text_size(ui_text_md(cx));

                            if !is_disabled {
                                row = row
                                    .cursor_pointer()
                                    .hover(|s| s.bg(p.surface_hover));
                            }

                            let row_left = div()
                                .flex()
                                .items_center()
                                .gap(SPACE_SM)
                                .min_w(px(0.0))
                                .flex_1()
                                .children(icon_el);
                            let row_left = if let Some(desc) = opt.description {
                                row_left.child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .min_w(px(0.0))
                                        .child(div().whitespace_nowrap().child(opt.label))
                                        .child(div().truncate().text_size(ui_text_xs(cx)).text_color(rgb(t.text_muted)).child(desc)),
                                )
                            } else {
                                row_left.child(div().whitespace_nowrap().child(opt.label))
                            };

                            let check_el = if is_selected {
                                Some(AppIcon::Check.size(ui_icon_sm(cx)).text_color(rgb(t.text_primary)))
                            } else {
                                None
                            };

                            row.child(row_left).children(check_el).into_any_element()
                        };

                        let val_clone = opt.value.clone();
                        let row_wrapper = div()
                            .w_full()
                            .child(row_el)
                            .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, window, cx| {
                                if !is_disabled {
                                    this.set_selected_value(Some(val_clone.clone()), cx);
                                    this.set_open(false, window, cx);
                                }
                            }));

                        scroll_area = scroll_area.child(row_wrapper);
                    }
                }

                let mut container = div()
                    .id("select-popover-list")
                    .relative()
                    .w_full()
                    .max_h(max_popover_h)
                    .child(scroll_area);

                if self.show_scrollbar {
                    container = container.child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .left_0()
                            .right_0()
                            .child(
                                Scrollbar::vertical(&self.scroll_handle)
                                    .scrollbar_show(ScrollbarShow::Hover),
                            ),
                    );
                }

                container
            };

            let registry_for_bounds = self.get_overlay_registry(cx);
            let overlay_id = self.overlay_id.clone();
            let trigger_bounds = self.trigger_bounds;

            let bounds_canvas = canvas(
                move |bounds: Bounds<Pixels>, _window: &mut Window, cx: &mut App| {
                    if let Some(reg) = &registry_for_bounds {
                        let combined_bounds = if trigger_bounds.size.width > px(0.0) {
                            Bounds::from_corners(
                                point(
                                    trigger_bounds.origin.x.min(bounds.origin.x),
                                    trigger_bounds.origin.y.min(bounds.origin.y),
                                ),
                                point(
                                    (trigger_bounds.origin.x + trigger_bounds.size.width)
                                        .max(bounds.origin.x + bounds.size.width),
                                    (trigger_bounds.origin.y + trigger_bounds.size.height)
                                        .max(bounds.origin.y + bounds.size.height),
                                ),
                            )
                        } else {
                            bounds
                        };
                        reg.update(cx, |r, _| {
                            r.set_bounds(&overlay_id, combined_bounds);
                        });
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0();

            let popover_inner = div()
                .child(bounds_canvas)
                .child(popover_list)
                .with_animation(
                    "select-popover-box",
                    gpui::Animation::new(Duration::from_millis(160))
                        .with_easing(|d| 1.0 - (1.0 - d).powi(3)),
                    |this, delta| this.opacity(delta),
                );

            let popover_box = div()
                .id("select-popover-box")
                .min_w(trigger_min_w)
                .when_some(popover_style_w, |d, w| d.w(w))
                .when_some(popover_style_max_w, |d, max_w| d.max_w(max_w))
                .relative()
                .occlude()
                .on_action(cx.listener(|this, _: &Cancel, window, cx| {
                    if this.open {
                        this.set_open(false, window, cx);
                    } else {
                        cx.propagate();
                    }
                }))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .bg(surface_bg(t.bg_primary, cx))
                .border_1()
                .border_color(p.border_subtle)
                .rounded(RADIUS_MD)
                .shadow(crate::tokens::elevation_menu_shadow())
                .child(popover_inner);

            let anchored_popover = if effective_above {
                anchored()
                    .position(point(
                        trigger_x,
                        trigger_y - SPACE_XS,
                    ))
                    .anchor(Anchor::BottomLeft)
                    .child(popover_box)
            } else {
                anchored()
                    .position(point(
                        trigger_x,
                        trigger_y + trigger_h + SPACE_XS,
                    ))
                    .anchor(Anchor::TopLeft)
                    .child(popover_box)
            };

            Some(deferred(anchored_popover))
        } else {
            None
        };

        div()
            .id("select-container")
            .w_full()
            .relative()
            .child(trigger)
            .children(popover_overlay)
    }
}

/// A wrapper element to easily render a [`SelectState`].
#[derive(IntoElement)]
pub struct Select<T: Clone + PartialEq + 'static> {
    state: Entity<SelectState<T>>,
}

impl<T: Clone + PartialEq + 'static> Select<T> {
    pub fn new(state: &Entity<SelectState<T>>) -> Self {
        Self {
            state: state.clone(),
        }
    }
}

impl<T: Clone + PartialEq + 'static> RenderOnce for Select<T> {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        self.state
    }
}

#[cfg(test)]
mod tests {
    use core::prelude::v1::test;
    use super::*;

    #[gpui::test]
    fn test_child_index_for_flat_index_no_headers(cx: &mut gpui::TestAppContext) {
        use gpui::AppContext as _;
        cx.update(|cx| velowork_i18n::init_locale(velowork_i18n::Locale::default(), cx));
        let select = cx.new(|cx| {
            SelectState::new(cx).options(vec![
                SelectOption::new("a", "Option A"),
                SelectOption::new("b", "Option B"),
                SelectOption::new("c", "Option C"),
            ])
        });

        select.read_with(cx, |this, _| {
            assert_eq!(this.child_index_for_flat_index(0), 0);
            assert_eq!(this.child_index_for_flat_index(1), 1);
            assert_eq!(this.child_index_for_flat_index(2), 2);
        });
    }

    #[gpui::test]
    fn test_child_index_for_flat_index_with_headers(cx: &mut gpui::TestAppContext) {
        use gpui::AppContext as _;
        cx.update(|cx| velowork_i18n::init_locale(velowork_i18n::Locale::default(), cx));
        let select = cx.new(|cx| {
            SelectState::new(cx).groups(vec![
                SelectGroup::new(vec![
                    SelectOption::new("a", "Option A"),
                    SelectOption::new("b", "Option B"),
                ])
                .title("Group 1"),
                SelectGroup::new(vec![
                    SelectOption::new("c", "Option C"),
                    SelectOption::new("d", "Option D"),
                ])
                .title("Group 2"),
            ])
        });

        select.read_with(cx, |this, _| {
            // Group 1 header is index 0.
            // Option A is index 1 (flat index 0).
            assert_eq!(this.child_index_for_flat_index(0), 1);
            // Option B is index 2 (flat index 1).
            assert_eq!(this.child_index_for_flat_index(1), 2);
            // Group 2 header is index 3.
            // Option C is index 4 (flat index 2).
            assert_eq!(this.child_index_for_flat_index(2), 4);
            // Option D is index 5 (flat index 3).
            assert_eq!(this.child_index_for_flat_index(3), 5);
        });
    }

    #[gpui::test]
    fn test_select_state_selection(cx: &mut gpui::TestAppContext) {
        use gpui::AppContext as _;
        cx.update(|cx| velowork_i18n::init_locale(velowork_i18n::Locale::default(), cx));
        let select = cx.new(|cx| {
            SelectState::new(cx)
                .options(vec![
                    SelectOption::new("light", "Light"),
                    SelectOption::new("dark", "Dark"),
                ])
                .selected(Some("dark"))
        });

        select.read_with(cx, |this, _| {
            assert_eq!(this.selected_value(), Some(&"dark"));
            let opt = this.selected_option().unwrap();
            assert_eq!(opt.label.as_ref(), "Dark");
        });
    }

    #[gpui::test]
    fn test_select_ghost_mode(cx: &mut gpui::TestAppContext) {
        use gpui::AppContext as _;
        cx.update(|cx| velowork_i18n::init_locale(velowork_i18n::Locale::default(), cx));
        let select = cx.new(|cx| {
            SelectState::<String>::new(cx)
                .ghost(true)
        });

        select.read_with(cx, |this, _| {
            assert!(this.ghost);
        });

        select.update(cx, |this, cx| {
            this.set_ghost(false, cx);
        });

        select.read_with(cx, |this, _| {
            assert!(!this.ghost);
        });
    }

    #[gpui::test]
    fn test_select_text_size(cx: &mut gpui::TestAppContext) {
        use gpui::AppContext as _;
        cx.update(|cx| velowork_i18n::init_locale(velowork_i18n::Locale::default(), cx));
        let select = cx.new(|cx| {
            SelectState::<String>::new(cx)
                .text_size(px(14.0))
        });

        select.read_with(cx, |this, _| {
            assert_eq!(this.text_size, Some(px(14.0)));
        });

        select.update(cx, |this, cx| {
            this.set_text_size(Some(px(16.0)), cx);
        });

        select.read_with(cx, |this, _| {
            assert_eq!(this.text_size, Some(px(16.0)));
        });
    }
}
