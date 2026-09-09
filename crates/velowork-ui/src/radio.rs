//! Universal Radio / RadioGroup Component (`velowork-ui::radio`).
//!
//! Supports two rendering modes:
//! 1. `RadioMode::Normal`: Standard radio button (circle indicator + label + description).
//! 2. `RadioMode::Button`: Segmented control button strip (used in settings panel etc.).

use std::rc::Rc;
use gpui::prelude::*;
use gpui::*;
use crate::design::appearance::{control_height_for_size, ControlSize};
use crate::design::semantic::SemanticPalette;
use crate::input::{focus_ring_shadows, InputFocusRingExt};
use crate::tokens::{ui_text_md, RADIUS_MD, RADIUS_XS, RADIUS_STD, SPACE_MD, SPACE_SM};
use crate::tooltip::Tooltip;

/// Mode for the Radio / RadioGroup component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RadioMode {
    /// 普通模式：圆圈/单选点 + 文字标签
    #[default]
    Normal,
    /// 按钮模式：分段按钮样式（用于设置界面等选项切换）
    Button,
}

/// A single option in a [`RadioGroup`].
#[derive(Clone)]
pub struct RadioOption<T: Clone + PartialEq + 'static> {
    pub value: T,
    pub label: SharedString,
    pub description: Option<SharedString>,
    pub tooltip: Option<SharedString>,
    pub disabled: bool,
}

impl<T: Clone + PartialEq + 'static> RadioOption<T> {
    pub fn new(value: T, label: impl Into<SharedString>) -> Self {
        Self {
            value,
            label: label.into(),
            description: None,
            tooltip: None,
            disabled: false,
        }
    }

    pub fn description(mut self, desc: impl Into<SharedString>) -> Self {
        self.description = Some(desc.into());
        self
    }

    pub fn tooltip(mut self, tip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tip.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

/// RadioGroup component.
#[derive(IntoElement)]
pub struct RadioGroup<T: Clone + PartialEq + 'static> {
    id: SharedString,
    options: Vec<RadioOption<T>>,
    selected_value: Option<T>,
    mode: RadioMode,
    size: ControlSize,
    full_width: bool,
    disabled: bool,
    focus_handle: Option<FocusHandle>,
    on_change: Option<Rc<dyn Fn(&T, &mut Window, &mut App)>>,
}

impl<T: Clone + PartialEq + 'static> RadioGroup<T> {
    pub fn new(id: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            options: Vec::new(),
            selected_value: None,
            mode: RadioMode::Normal,
            size: ControlSize::Default,
            full_width: false,
            disabled: false,
            focus_handle: None,
            on_change: None,
        }
    }

    pub fn focus(mut self, handle: &FocusHandle) -> Self {
        self.focus_handle = Some(handle.clone());
        self
    }

    pub fn focus_opt(mut self, handle: Option<&FocusHandle>) -> Self {
        self.focus_handle = handle.cloned();
        self
    }

    pub fn options(mut self, options: Vec<RadioOption<T>>) -> Self {
        self.options = options;
        self
    }

    pub fn selected(mut self, selected: Option<T>) -> Self {
        self.selected_value = selected;
        self
    }

    pub fn mode(mut self, mode: RadioMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }

    pub fn compact(mut self) -> Self {
        self.size = ControlSize::Compact;
        self
    }

    pub fn full_width(mut self, full: bool) -> Self {
        self.full_width = full;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_change(
        mut self,
        handler: impl Fn(&T, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl<T: Clone + PartialEq + 'static> RenderOnce for RadioGroup<T> {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = crate::theme::theme(cx);
        let p = SemanticPalette::from_context(cx);

        let options_values: Vec<T> = self.options.iter().map(|o| o.value.clone()).collect();
        let selected_val = self.selected_value.clone();
        let on_change_kd = self.on_change.clone();
        let focus_handle = self.focus_handle.clone();
        let is_focused = focus_handle.as_ref().is_some_and(|fh| fh.is_focused(window));

        match self.mode {
            RadioMode::Button => {
                let is_compact = self.size == ControlSize::Compact;
                let h_px = control_height_for_size(self.size, cx);

                let mut container = div()
                    .id(self.id.clone())
                    .flex()
                    .items_center()
                    .gap(px(2.0))
                    .rounded(RADIUS_STD)
                    .bg(p.surface_card)
                    .border_1()
                    .border_color(p.border_subtle)
                    .p(px(2.0))
                    .h(h_px)
                    .when(!self.full_width, |el| el.self_start())
                    .when(self.full_width, |el| el.w_full());

                if let Some(fh) = focus_handle.as_ref() {
                    container = container.track_focus(fh).focus_ring_on(fh, &t);
                }

                if !self.disabled {
                    if let Some(on_change) = on_change_kd {
                        container = container.on_key_down(move |event, window, cx| {
                            let key = event.keystroke.key.as_str();
                            if options_values.is_empty() {
                                return;
                            }
                            let curr_idx = selected_val
                                .as_ref()
                                .and_then(|v| options_values.iter().position(|ov| ov == v))
                                .unwrap_or(0);
                            let len = options_values.len();
                            if matches!(key, "arrow_right" | "right" | "arrow_down" | "down") {
                                cx.stop_propagation();
                                let next_idx = (curr_idx + 1) % len;
                                (on_change)(&options_values[next_idx], window, cx);
                            } else if matches!(key, "arrow_left" | "left" | "arrow_up" | "up") {
                                cx.stop_propagation();
                                let prev_idx = (curr_idx + len - 1) % len;
                                (on_change)(&options_values[prev_idx], window, cx);
                            }
                        });
                    }
                }

                if self.disabled {
                    container = container.opacity(0.5).cursor_not_allowed();
                }

                for (idx, opt) in self.options.into_iter().enumerate() {
                    let is_selected = self.selected_value.as_ref() == Some(&opt.value);
                    let is_disabled = self.disabled || opt.disabled;
                    let val = opt.value.clone();
                    let on_change = self.on_change.clone();
                    let fh_click = focus_handle.clone();

                    if idx > 0 {
                        container = container.child(
                            div()
                                .w(px(1.0))
                                .h_full()
                                .flex_shrink_0()
                                .bg(p.border_subtle),
                        );
                    }

                    let hover_bg = p.surface_hover;

                    let mut btn = div()
                        .id(ElementId::Name(format!("{}-btn-{}", self.id, idx).into()))
                        .h_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(!self.full_width, |el| el.flex_shrink_0())
                        .when(self.full_width, |el| el.flex_1())
                        .when(is_compact, |el| {
                            el.px(SPACE_SM)
                                .rounded(RADIUS_XS)
                                .text_size(ui_text_md(cx))
                        })
                        .when(!is_compact, |el| {
                            el.px(SPACE_MD)
                                .rounded(RADIUS_MD)
                                .text_size(ui_text_md(cx))
                        });

                    if is_selected {
                        btn = btn
                            .bg(p.surface_selection)
                            .border_1()
                            .border_color(p.border_subtle)
                            .text_color(p.text_primary);
                    } else {
                        btn = btn.text_color(p.text_muted);
                        if !is_disabled {
                            btn = btn
                                .cursor_pointer()
                                .hover(|s| s.bg(hover_bg).text_color(p.text_primary));
                        }
                    }

                    if !is_disabled {
                        if let Some(handler) = on_change {
                            btn = btn.on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                if let Some(fh) = fh_click.as_ref() {
                                    window.focus(fh, cx);
                                }
                                handler(&val, window, cx);
                            });
                        }
                    }

                    if let Some(tip) = opt.tooltip.clone() {
                        btn = btn.tooltip(move |_, cx| {
                            let t = tip.clone();
                            cx.new(|_| Tooltip::new(t)).into()
                        });
                    }

                    container = container.child(btn.child(opt.label));
                }

                container.into_any_element()
            }
            RadioMode::Normal => {
                let mut container = div()
                    .id(self.id.clone())
                    .flex()
                    .flex_col()
                    .gap(SPACE_MD)
                    .when(!self.full_width, |el| el.self_start())
                    .when(self.full_width, |el| el.w_full());

                if let Some(fh) = focus_handle.as_ref() {
                    container = container.track_focus(fh);
                }

                if !self.disabled {
                    if let Some(on_change) = on_change_kd {
                        container = container.on_key_down(move |event, window, cx| {
                            let key = event.keystroke.key.as_str();
                            if options_values.is_empty() {
                                return;
                            }
                            let curr_idx = selected_val
                                .as_ref()
                                .and_then(|v| options_values.iter().position(|ov| ov == v))
                                .unwrap_or(0);
                            let len = options_values.len();
                            if matches!(key, "arrow_right" | "right" | "arrow_down" | "down") {
                                cx.stop_propagation();
                                let next_idx = (curr_idx + 1) % len;
                                (on_change)(&options_values[next_idx], window, cx);
                            } else if matches!(key, "arrow_left" | "left" | "arrow_up" | "up") {
                                cx.stop_propagation();
                                let prev_idx = (curr_idx + len - 1) % len;
                                (on_change)(&options_values[prev_idx], window, cx);
                            }
                        });
                    }
                }

                if self.disabled {
                    container = container.opacity(0.5);
                }

                for (idx, opt) in self.options.into_iter().enumerate() {
                    let is_selected = self.selected_value.as_ref() == Some(&opt.value);
                    let is_disabled = self.disabled || opt.disabled;
                    let val = opt.value.clone();
                    let on_change = self.on_change.clone();
                    let opt_tip = opt.tooltip.clone();
                    let fh_click = focus_handle.clone();

                    let circle_border = if is_selected {
                        p.border_active
                    } else {
                        p.border_subtle
                    };

                    let circle_bg = if is_selected {
                        p.surface_selection
                    } else {
                        p.surface_card
                    };

                    let circle_dot = if is_selected {
                        Some(
                            div()
                                .w(px(8.0))
                                .h(px(8.0))
                                .rounded_full()
                                .bg(p.surface_accent),
                        )
                    } else {
                        None
                    };

                    let grp_id: SharedString = format!("{}-radio-grp-{}", self.id, idx).into();

                    let mut item = div()
                        .id(ElementId::Name(format!("{}-item-{}", self.id, idx).into()))
                        .group(grp_id.clone())
                        .flex()
                        .items_start()
                        .gap(SPACE_MD);

                    if !is_disabled {
                        item = item.cursor_pointer();
                        if let Some(handler) = on_change {
                            item = item.on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                if let Some(fh) = fh_click.as_ref() {
                                    window.focus(fh, cx);
                                }
                                handler(&val, window, cx);
                            });
                        }
                    }

                    if let Some(tip) = opt_tip {
                        item = item.tooltip(move |_, cx| {
                            let t = tip.clone();
                            cx.new(|_| Tooltip::new(t)).into()
                        });
                    }

                    let circle = div()
                        .w(px(16.0))
                        .h(px(16.0))
                        .flex_shrink_0()
                        .mt(px(2.0))
                        .rounded_full()
                        .border_1()
                        .border_color(circle_border)
                        .bg(circle_bg)
                        .when(is_selected && is_focused && !self.disabled, |c| {
                            c.shadow(focus_ring_shadows(&t))
                        })
                        .when(!is_selected && !is_disabled && !is_focused, |c| {
                            c.group_hover(grp_id, |s| {
                                s.border_color(p.surface_accent.opacity(0.6)).bg(p.surface_hover)
                            })
                        })
                        .flex()
                        .items_center()
                        .justify_center()
                        .children(circle_dot);

                    let label_col = if let Some(desc) = opt.description {
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .text_size(ui_text_md(cx))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(if is_selected { t.text_primary } else { t.text_secondary }))
                                    .child(opt.label),
                            )
                            .child(
                                div()
                                    .text_size(ui_text_md(cx))
                                    .text_color(rgb(t.text_muted))
                                    .child(desc),
                            )
                    } else {
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(ui_text_md(cx))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(if is_selected { t.text_primary } else { t.text_secondary }))
                                    .child(opt.label),
                            )
                    };

                    item = item.child(circle).child(label_col);
                    container = container.child(item);
                }

                container.into_any_element()
            }
        }
    }
}
