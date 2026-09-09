//! Reusable `Slider` component for Velowork UI.
//!
//! Supports single-value and range-slider modes, horizontal and vertical orientation,
//! linear and logarithmic scales, keyboard navigation, and full theme integration.

use std::ops::Range;
use gpui::*;
use gpui::prelude::*;

use crate::theme::theme;

/// Events emitted by [`SliderState`].
#[derive(Clone, Copy, Debug)]
pub enum SliderEvent {
    /// Emitted continuously while the slider value is being changed by the user.
    Change(SliderValue),
    /// Emitted once when the user releases the slider thumb after a drag or click.
    Release(SliderValue),
}

/// The value of a [`Slider`], supporting either a single value or a value range.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SliderValue {
    Single(f32),
    Range(f32, f32),
}

impl std::fmt::Display for SliderValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SliderValue::Single(value) => write!(f, "{:.2}", value),
            SliderValue::Range(start, end) => write!(f, "{:.2}..{:.2}", start, end),
        }
    }
}

impl From<f32> for SliderValue {
    fn from(value: f32) -> Self {
        SliderValue::Single(value)
    }
}

impl From<(f32, f32)> for SliderValue {
    fn from(value: (f32, f32)) -> Self {
        SliderValue::Range(value.0, value.1)
    }
}

impl From<Range<f32>> for SliderValue {
    fn from(value: Range<f32>) -> Self {
        SliderValue::Range(value.start, value.end)
    }
}

impl Default for SliderValue {
    fn default() -> Self {
        SliderValue::Single(0.0)
    }
}

impl SliderValue {
    pub fn clamp(self, min: f32, max: f32) -> Self {
        match self {
            SliderValue::Single(value) => SliderValue::Single(value.clamp(min, max)),
            SliderValue::Range(start, end) => {
                SliderValue::Range(start.clamp(min, max), end.clamp(min, max))
            }
        }
    }

    pub fn is_single(&self) -> bool {
        matches!(self, SliderValue::Single(_))
    }

    pub fn is_range(&self) -> bool {
        matches!(self, SliderValue::Range(_, _))
    }

    pub fn start(&self) -> f32 {
        match self {
            SliderValue::Single(value) => *value,
            SliderValue::Range(start, _) => *start,
        }
    }

    pub fn end(&self) -> f32 {
        match self {
            SliderValue::Single(value) => *value,
            SliderValue::Range(_, end) => *end,
        }
    }

    pub fn set_start(&mut self, value: f32) {
        if let SliderValue::Range(_, end) = self {
            *self = SliderValue::Range(value.min(*end), *end);
        } else {
            *self = SliderValue::Single(value);
        }
    }

    pub fn set_end(&mut self, value: f32) {
        if let SliderValue::Range(start, _) = self {
            *self = SliderValue::Range(*start, value.max(*start));
        } else {
            *self = SliderValue::Single(value);
        }
    }
}

/// Scale progression mode of the slider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SliderScale {
    #[default]
    Linear,
    Logarithmic,
}

/// Observable state entity for a [`Slider`].
pub struct SliderState {
    min: f32,
    max: f32,
    step: f32,
    value: SliderValue,
    percentage: Range<f32>,
    bounds: Bounds<Pixels>,
    scale: SliderScale,
    dragging: bool,
    active_thumb: Option<bool>,
    focus_handle: FocusHandle,
}

impl EventEmitter<SliderEvent> for SliderState {}

impl SliderState {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        Self {
            min: 0.0,
            max: 100.0,
            step: 1.0,
            value: SliderValue::default(),
            percentage: 0.0..0.0,
            bounds: Bounds::default(),
            scale: SliderScale::Linear,
            dragging: false,
            active_thumb: None,
            focus_handle,
        }
    }

    pub fn min(mut self, min: f32) -> Self {
        self.min = min;
        self.update_thumb_pos();
        self
    }

    pub fn max(mut self, max: f32) -> Self {
        self.max = max;
        self.update_thumb_pos();
        self
    }

    pub fn step(mut self, step: f32) -> Self {
        self.step = step;
        self
    }

    pub fn scale(mut self, scale: SliderScale) -> Self {
        self.scale = scale;
        self.update_thumb_pos();
        self
    }

    pub fn value(mut self, value: impl Into<SliderValue>) -> Self {
        self.value = value.into();
        self.update_thumb_pos();
        self
    }

    pub fn get_value(&self) -> SliderValue {
        self.value
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }

    pub fn is_dragging(&self) -> bool {
        self.dragging
    }

    pub fn set_value(&mut self, value: impl Into<SliderValue>, cx: &mut Context<Self>) {
        let val = value.into().clamp(self.min, self.max);
        if self.value != val {
            self.value = val;
            self.update_thumb_pos();
            cx.emit(SliderEvent::Change(self.value));
            cx.notify();
        }
    }

    fn percentage_to_value(&self, percentage: f32) -> f32 {
        match self.scale {
            SliderScale::Linear => self.min + (self.max - self.min) * percentage,
            SliderScale::Logarithmic => {
                if self.min <= 0.0 {
                    self.min + (self.max - self.min) * percentage
                } else {
                    let base = self.max / self.min;
                    (base.powf(percentage) * self.min).clamp(self.min, self.max)
                }
            }
        }
    }

    fn value_to_percentage(&self, value: f32) -> f32 {
        match self.scale {
            SliderScale::Linear => {
                let range = self.max - self.min;
                if range <= 0.0 {
                    0.0
                } else {
                    (value - self.min) / range
                }
            }
            SliderScale::Logarithmic => {
                if self.min <= 0.0 {
                    let range = self.max - self.min;
                    if range <= 0.0 { 0.0 } else { (value - self.min) / range }
                } else {
                    let base = self.max / self.min;
                    (value / self.min).log(base).clamp(0.0, 1.0)
                }
            }
        }
    }

    fn update_thumb_pos(&mut self) {
        match self.value {
            SliderValue::Single(value) => {
                let percentage = self.value_to_percentage(value.clamp(self.min, self.max));
                self.percentage = 0.0..percentage;
            }
            SliderValue::Range(start, end) => {
                let clamped_start = start.clamp(self.min, self.max);
                let clamped_end = end.clamp(self.min, self.max);
                self.percentage =
                    self.value_to_percentage(clamped_start)..self.value_to_percentage(clamped_end);
            }
        }
    }

    fn update_value_by_position(
        &mut self,
        axis: Axis,
        position: Point<Pixels>,
        is_start: bool,
        cx: &mut Context<Self>,
    ) {
        self.dragging = true;
        self.active_thumb = Some(is_start);
        let bounds = self.bounds;
        let step = self.step;

        let inner_pos = if axis == Axis::Horizontal {
            position.x - bounds.left()
        } else {
            bounds.bottom() - position.y
        };

        let total_size = if axis == Axis::Horizontal {
            bounds.size.width
        } else {
            bounds.size.height
        };

        if total_size <= px(0.0) {
            return;
        }

        let raw_percentage = (inner_pos / total_size).clamp(0.0, 1.0);
        let percentage = if is_start {
            raw_percentage.clamp(0.0, self.percentage.end)
        } else {
            raw_percentage.clamp(self.percentage.start, 1.0)
        };

        let raw_val = self.percentage_to_value(percentage);
        let value = if step > 0.0 {
            let steps = ((raw_val - self.min) / step).round();
            (self.min + steps * step).clamp(self.min, self.max)
        } else {
            raw_val.clamp(self.min, self.max)
        };

        let mut changed = false;
        if is_start {
            let old_start = self.value.start();
            self.value.set_start(value);
            if self.value.start() != old_start {
                changed = true;
            }
        } else {
            let old_end = self.value.end();
            self.value.set_end(value);
            if self.value.end() != old_end {
                changed = true;
            }
        }

        self.update_thumb_pos();

        if changed {
            cx.emit(SliderEvent::Change(self.value));
        }
        cx.notify();
    }

    pub fn handle_release(&mut self, cx: &mut Context<Self>) {
        if self.dragging {
            self.dragging = false;
            self.active_thumb = None;
            cx.emit(SliderEvent::Release(self.value));
            cx.notify();
        }
    }

    fn handle_key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let delta = match event.keystroke.key.as_str() {
            "left" | "down" => -self.step,
            "right" | "up" => self.step,
            "page_down" => -self.step * 10.0,
            "page_up" => self.step * 10.0,
            "home" => {
                let min = self.min;
                self.set_value(min, cx);
                return;
            }
            "end" => {
                let max = self.max;
                self.set_value(max, cx);
                return;
            }
            _ => return,
        };

        let current = self.value.end();
        let new_val = (current + delta).clamp(self.min, self.max);
        self.set_value(new_val, cx);
    }
}

/// Element for rendering a [`SliderState`].
#[derive(IntoElement)]
pub struct Slider {
    state: Entity<SliderState>,
    axis: Axis,
    disabled: bool,
}

impl Slider {
    pub fn new(state: &Entity<SliderState>) -> Self {
        Self {
            state: state.clone(),
            axis: Axis::Horizontal,
            disabled: false,
        }
    }

    pub fn horizontal(mut self) -> Self {
        self.axis = Axis::Horizontal;
        self
    }

    pub fn vertical(mut self) -> Self {
        self.axis = Axis::Vertical;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl RenderOnce for Slider {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let _t = theme(cx);
        let axis = self.axis;
        let disabled = self.disabled;

        let state_read = self.state.read(cx);
        let percentage = state_read.percentage.clone();
        let is_range = state_read.value.is_range();
        let focus_handle = state_read.focus_handle.clone();
        let is_focused = focus_handle.is_focused(_window);

        let p = crate::design::semantic::SemanticPalette::from_context(cx);
        let active_bar_color = if is_focused || state_read.dragging {
            p.surface_accent
        } else {
            p.text_primary
        };
        let track_bg_color = p.surface_card;
        let thumb_bg_color = p.surface_base;

        let mouse_listener = canvas(
            {
                let bounds_entity = self.state.clone();
                move |bounds: Bounds<Pixels>, _window: &mut Window, cx: &mut App| {
                    bounds_entity.update(cx, |this, _| {
                        this.bounds = bounds;
                    });
                }
            },
            {
                let state_entity = self.state.clone();
                move |_bounds: Bounds<Pixels>, _prepaint: (), window: &mut Window, cx: &mut App| {
                    let state_read = state_entity.read(cx);
                    let is_dragging = state_read.dragging;
                    let is_start = state_read.active_thumb.unwrap_or(false);

                    if is_dragging && !disabled {
                        let state_move = state_entity.clone();
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _window, cx| {
                            if phase.bubble() {
                                state_move.update(cx, |state, cx| {
                                    if state.dragging {
                                        state.update_value_by_position(axis, event.position, is_start, cx);
                                    }
                                });
                            }
                        });

                        let state_up = state_entity.clone();
                        window.on_mouse_event(move |event: &MouseUpEvent, phase, _window, cx| {
                            if phase.bubble() && event.button == MouseButton::Left {
                                state_up.update(cx, |state, cx| {
                                    state.handle_release(cx);
                                });
                            }
                        });
                    }
                }
            },
        );

        // Filled active track progress
        let active_track = div()
            .absolute()
            .bg(active_bar_color)
            .rounded_full()
            .when(axis == Axis::Horizontal, |this| {
                this.h_full()
                    .left(relative(percentage.start))
                    .right(relative(1.0 - percentage.end))
            })
            .when(axis == Axis::Vertical, |this| {
                this.w_full()
                    .bottom(relative(percentage.start))
                    .top(relative(1.0 - percentage.end))
            });

        // Track bar background
        let mut track_bar = div()
            .id("slider-track-bar")
            .absolute()
            .bg(track_bg_color)
            .border_1()
            .border_color(p.border_subtle)
            .rounded_full()
            .child(active_track);

        if axis == Axis::Horizontal {
            track_bar = track_bar.w_full().h(px(4.0)).top(px(6.0)).left(px(0.0));
        } else {
            track_bar = track_bar.h_full().w(px(4.0)).left(px(6.0)).top(px(0.0));
        }

        // Thumb render helper
        let render_thumb = |pct: f32, is_start: bool| {
            let entity_state = self.state.clone();
            let is_active = state_read.dragging && state_read.active_thumb == Some(is_start);
            let border_col = if is_active {
                p.border_active
            } else {
                active_bar_color
            };

            let mut thumb = div()
                .id(ElementId::Name(format!("slider-thumb-{}", is_start as u32).into()))
                .absolute()
                .size(px(14.0))
                .rounded_full()
                .bg(thumb_bg_color)
                .border_2()
                .border_color(border_col)
                .shadow_sm()
                .cursor_pointer();

            if axis == Axis::Horizontal {
                thumb = thumb.top(px(1.0)).left(relative(pct)).ml(px(-7.0));
            } else {
                thumb = thumb.left(px(1.0)).bottom(relative(pct)).mb(px(-7.0));
            }

            if !disabled {
                thumb = thumb
                    .hover(|s| s.bg(p.surface_hover))
                    .on_mouse_down(MouseButton::Left, move |e, _window, cx| {
                        cx.stop_propagation();
                        entity_state.update(cx, |state, cx| {
                            state.update_value_by_position(axis, e.position, is_start, cx);
                        });
                    });
            }

            thumb
        };

        let thumbs = if is_range {
            div()
                .absolute()
                .inset_0()
                .child(render_thumb(percentage.start, true))
                .child(render_thumb(percentage.end, false))
        } else {
            div()
                .absolute()
                .inset_0()
                .child(render_thumb(percentage.end, false))
        };

        // Main track container
        let mut track_wrapper = div()
            .id("slider-track-wrapper")
            .relative()
            .child(track_bar)
            .child(mouse_listener.absolute().inset_0())
            .child(thumbs);

        if axis == Axis::Horizontal {
            track_wrapper = track_wrapper.w_full().h(px(16.0));
        } else {
            track_wrapper = track_wrapper.h(px(120.0)).w(px(16.0));
        }

        if !disabled {
            let entity_state = self.state.clone();
            track_wrapper = track_wrapper.on_mouse_down(MouseButton::Left, move |e, _window, cx| {
                cx.stop_propagation();
                entity_state.update(cx, |state, cx| {
                    let is_start = if is_range {
                        let total_size = if axis == Axis::Horizontal {
                            state.bounds.size.width
                        } else {
                            state.bounds.size.height
                        };
                        let inner_pos = if axis == Axis::Horizontal {
                            e.position.x - state.bounds.left()
                        } else {
                            state.bounds.bottom() - e.position.y
                        };
                        let click_pct = (inner_pos / total_size).clamp(0.0, 1.0);
                        let dist_to_start = (click_pct - state.percentage.start).abs();
                        let dist_to_end = (click_pct - state.percentage.end).abs();
                        dist_to_start < dist_to_end
                    } else {
                        false
                    };
                    state.update_value_by_position(axis, e.position, is_start, cx);
                });
            });
        }

        let entity_state_rel = self.state.clone();
        let entity_state_key = self.state.clone();

        let mut container = div()
            .id("slider-container")
            .track_focus(&focus_handle)
            .flex()
            .items_center()
            .justify_center()
            .p(px(6.0))
            .when(disabled, |d| d.opacity(0.5).cursor_not_allowed())
            .when(!disabled, |c| {
                c.on_mouse_up(MouseButton::Left, move |_, _window, cx| {
                    entity_state_rel.update(cx, |s, cx| s.handle_release(cx));
                })
                .on_key_down(move |e, _window, cx| {
                    entity_state_key.update(cx, |s, cx| s.handle_key_down(e, cx));
                })
            })
            .child(track_wrapper);

        if axis == Axis::Horizontal {
            container = container.w_full();
        } else {
            container = container.h(px(132.0));
        }

        container
    }
}

