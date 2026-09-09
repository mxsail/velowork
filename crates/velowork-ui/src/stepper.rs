//! Generic Number Stepper component with [-] [+] buttons and editable text input in the middle.
//!
//! Inspired by Ant Design's InputNumber component:
//! - Centered text in the middle input box
//! - Min / Max bounds validation
//! - Precision / decimal places formatting
//! - Configurable step size
//! - Custom formatter and parser (e.g. adding `%` or `px` suffix)

use std::sync::Arc;

use gpui::prelude::FluentBuilder;
use gpui::*;
use crate::design::semantic::SemanticPalette;
use crate::h_flex;
use crate::input::{focus_ring_shadows, InputState};
use crate::theme::ThemeColors;
use crate::tokens::{ui_text_xl, RADIUS_MD};

pub type FormatterFn = Arc<dyn Fn(f32) -> String + Send + Sync>;
pub type ParserFn = Arc<dyn Fn(&str) -> Option<f32> + Send + Sync>;
pub type CommitFn = Arc<dyn Fn(&str, &mut Window, &mut App) + 'static>;

/// A reusable number stepper component ([-] [ editable text ] [+]).
#[derive(IntoElement)]
pub struct NumberStepper {
    id: ElementId,
    width: Option<Pixels>,
    input_state: Entity<InputState>,
    value: f32,
    min: Option<f32>,
    max: Option<f32>,
    step: f32,
    precision: Option<usize>,
    formatter: Option<FormatterFn>,
    parser: Option<ParserFn>,
    on_dec: Option<Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
    on_inc: Option<Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
    on_commit: Option<CommitFn>,
    on_change: Option<Box<dyn Fn(f32, &mut Window, &mut App) + 'static>>,
    t: ThemeColors,
}

impl NumberStepper {
    pub fn new(
        id: impl Into<ElementId>,
        input_state: Entity<InputState>,
        t: &ThemeColors,
    ) -> Self {
        Self {
            id: id.into(),
            width: None,
            input_state,
            value: 0.0,
            min: None,
            max: None,
            step: 1.0,
            precision: None,
            formatter: None,
            parser: None,
            on_dec: None,
            on_inc: None,
            on_commit: None,
            on_change: None,
            t: *t,
        }
    }

    pub fn width(mut self, width: impl Into<Pixels>) -> Self {
        self.width = Some(width.into());
        self
    }

    pub fn value(mut self, val: f32) -> Self {
        self.value = val;
        self
    }

    pub fn min(mut self, min: f32) -> Self {
        self.min = Some(min);
        self
    }

    pub fn max(mut self, max: f32) -> Self {
        self.max = Some(max);
        self
    }

    pub fn step(mut self, step: f32) -> Self {
        self.step = step;
        self
    }

    pub fn precision(mut self, precision: usize) -> Self {
        self.precision = Some(precision);
        self
    }

    pub fn formatter(mut self, f: impl Fn(f32) -> String + Send + Sync + 'static) -> Self {
        self.formatter = Some(Arc::new(f));
        self
    }

    pub fn parser(mut self, p: impl Fn(&str) -> Option<f32> + Send + Sync + 'static) -> Self {
        self.parser = Some(Arc::new(p));
        self
    }

    pub fn on_dec<F>(mut self, listener: F) -> Self
    where
        F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    {
        self.on_dec = Some(Box::new(listener));
        self
    }

    pub fn on_inc<F>(mut self, listener: F) -> Self
    where
        F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    {
        self.on_inc = Some(Box::new(listener));
        self
    }

    pub fn on_commit<F>(mut self, listener: F) -> Self
    where
        F: Fn(&str, &mut Window, &mut App) + 'static,
    {
        self.on_commit = Some(Arc::new(listener));
        self
    }

    pub fn on_change<F>(mut self, listener: F) -> Self
    where
        F: Fn(f32, &mut Window, &mut App) + 'static,
    {
        self.on_change = Some(Box::new(listener));
        self
    }

    pub fn clamp_and_round(&self, val: f32) -> f32 {
        let mut v = val;
        if let Some(min) = self.min {
            v = v.max(min);
        }
        if let Some(max) = self.max {
            v = v.min(max);
        }
        if let Some(p) = self.precision {
            let factor = 10.0_f32.powi(p as i32);
            v = (v * factor).round() / factor;
        }
        v
    }

    pub fn format_number(&self, val: f32) -> String {
        let v = self.clamp_and_round(val);
        if let Some(ref f) = self.formatter {
            f(v)
        } else if let Some(p) = self.precision {
            format!("{:.1$}", v, p)
        } else if v.fract() == 0.0 {
            format!("{:.0}", v)
        } else {
            format!("{:.1}", v)
        }
    }

    pub fn parse_number(&self, text: &str) -> Option<f32> {
        let parsed = if let Some(ref p) = self.parser {
            p(text)
        } else {
            let cleaned = text.trim_end_matches(|c: char| !c.is_numeric() && c != '.' && c != '-').trim();
            cleaned.parse::<f32>().ok()
        };
        parsed.map(|v| self.clamp_and_round(v))
    }
}


impl RenderOnce for NumberStepper {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = self.t;
        let p = SemanticPalette::from_theme(&t);
        let id = self.id.clone();
        let input_state = self.input_state.clone();
        let on_commit = self.on_commit;
        let dec_id = format!("{:?}-dec", id);
        let inc_id = format!("{:?}-inc", id);

        let focus_handle = input_state.read(cx).focus_handle(cx);
        let is_focused = focus_handle.is_focused(window);
        let ring = focus_ring_shadows(&t);

        // Ensure text alignment is set to center
        input_state.update(cx, |s, cx| {
            s.set_text_align(TextAlign::Center, cx);
        });

        let ctrl_h = crate::design::appearance::control_height(cx);

        let dec_btn = div()
            .id(ElementId::Name(dec_id.into()))
            .cursor_pointer()
            .w(ctrl_h)
            .h(ctrl_h)
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .overflow_hidden()
            .hover(|s| s.bg(p.surface_hover).text_color(p.text_primary))
            .text_size(ui_text_xl(cx))
            .text_color(p.text_secondary)
            .child("-");

        let dec_btn = if let Some(on_dec) = self.on_dec {
            dec_btn.on_click(move |ev, window, cx| on_dec(ev, window, cx))
        } else {
            dec_btn
        };

        let inc_btn = div()
            .id(ElementId::Name(inc_id.into()))
            .cursor_pointer()
            .w(ctrl_h)
            .h(ctrl_h)
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .overflow_hidden()
            .hover(|s| s.bg(p.surface_hover).text_color(p.text_primary))
            .text_size(ui_text_xl(cx))
            .text_color(p.text_secondary)
            .child("+");

        let inc_btn = if let Some(on_inc) = self.on_inc {
            inc_btn.on_click(move |ev, window, cx| on_inc(ev, window, cx))
        } else {
            inc_btn
        };

        let input_state_focus = self.input_state.clone();
        let mut input_box = div()
            .h(ctrl_h)
            .flex()
            .items_center()
            .overflow_hidden()
            .text_color(p.text_primary)
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                cx.stop_propagation();
                input_state_focus.update(cx, |s, cx| s.focus(window, cx));
            })
            .child(
                self.input_state.clone()
            )
            .on_key_down(move |e: &KeyDownEvent, window, cx| {
                if e.keystroke.key == "enter" {
                    if let Some(ref on_commit) = on_commit {
                        let text = input_state.read(cx).text().to_string();
                        on_commit(&text, window, cx);
                    }
                }
            });

        if let Some(w) = self.width {
            input_box = input_box.w(w);
        } else {
            input_box = input_box.flex_1().w_full();
        }

        let left_divider = div()
            .w(px(1.0))
            .h(ctrl_h)
            .bg(if is_focused {
                p.border_active.opacity(0.3)
            } else {
                p.border_subtle
            })
            .flex_shrink_0();

        let right_divider = div()
            .w(px(1.0))
            .h(ctrl_h)
            .bg(if is_focused {
                p.border_active.opacity(0.3)
            } else {
                p.border_subtle
            })
            .flex_shrink_0();

        let mut container = h_flex()
            .items_center()
            .h(ctrl_h)
            .rounded(RADIUS_MD)
            .bg(if is_focused {
                p.surface_hover
            } else {
                p.surface_card
            })
            .border_1()
            .border_color(if is_focused {
                p.border_active
            } else {
                p.border_subtle
            })
            .when(is_focused, |s| s.shadow(ring))
            .when(!is_focused, |s| {
                s.hover(|h| {
                    h.border_color(p.surface_accent.opacity(0.6))
                        .bg(p.surface_hover)
                })
            })
            .track_focus(&focus_handle)
            .overflow_hidden();

        if self.width.is_none() {
            container = container.w_full().flex_1();
        }

        container
            .child(dec_btn)
            .child(left_divider)
            .child(input_box)
            .child(right_divider)
            .child(inc_btn)
    }
}

/// Helper function to build a NumberStepper.
pub fn number_stepper(
    id: impl Into<ElementId>,
    input_state: Entity<InputState>,
    t: &ThemeColors,
) -> NumberStepper {
    NumberStepper::new(id, input_state, t)
}


