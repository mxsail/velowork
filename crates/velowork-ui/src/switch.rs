//! Native Switch (Toggle Switch) component for Velowork.

use std::rc::Rc;
use crate::design::semantic::SemanticPalette;
use crate::theme::{surface_bg, theme, with_alpha};
use gpui::prelude::*;
use gpui::*;

#[derive(IntoElement)]
pub struct Switch {
    id: ElementId,
    checked: bool,
    disabled: bool,
    focus_handle: Option<FocusHandle>,
    active_color: Option<u32>,
    on_click: Option<Rc<dyn Fn(&bool, &mut Window, &mut App) + 'static>>,
}

impl Switch {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            checked: false,
            disabled: false,
            focus_handle: None,
            active_color: None,
            on_click: None,
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

    pub fn active_color(mut self, color: u32) -> Self {
        self.active_color = Some(color);
        self
    }

    pub fn active_color_opt(mut self, color: Option<u32>) -> Self {
        self.active_color = color;
        self
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Switch {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_context(cx);

        let h_val = 20.0f32;
        let track_w = px(34.0);
        let track_h = px(h_val);
        let knob_size = px(14.0);
        let radius = px(10.0);
        let knob_offset_on = px(15.0);
        let knob_offset_off = px(3.0);

        let track_bg = if self.checked {
            if let Some(c) = self.active_color {
                surface_bg(c, cx)
            } else {
                p.surface_accent
            }
        } else {
            p.surface_card
        };

        let track_border = if self.checked {
            if let Some(c) = self.active_color {
                rgb(c).into()
            } else {
                p.border_active
            }
        } else {
            p.border_subtle
        };

        let is_focused = self
            .focus_handle
            .as_ref()
            .map(|fh| fh.is_focused(window))
            .unwrap_or(false);

        let mut el = div()
            .id(self.id)
            .cursor_pointer()
            .w(track_w)
            .h(track_h)
            .rounded(radius)
            .bg(track_bg)
            .border_1()
            .border_color(if is_focused { p.border_active } else { track_border })
            .when(!self.checked && !is_focused, |s| {
                s.hover(|h| {
                    h.border_color(p.surface_accent.opacity(0.6))
                        .bg(p.surface_hover)
                })
            })
            .when(self.checked && !is_focused, |s| {
                s.hover(|h| h.opacity(0.9))
            })
            .flex()
            .items_center()
            .child(
                div()
                    .w(knob_size)
                    .h(knob_size)
                    .rounded_full()
                    .bg(p.text_primary)
                    .ml(if self.checked { knob_offset_on } else { knob_offset_off }),
            );

        if let Some(fh) = self.focus_handle.as_ref() {
            let fh_click = fh.clone();
            el = el
                .track_focus(fh)
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    window.focus(&fh_click, cx);
                })
                .when(is_focused, |el| {
                    el.shadow(vec![BoxShadow {
                        color: with_alpha(t.border_active, 0.35),
                        offset: point(px(0.0), px(0.0)),
                        blur_radius: px(3.0),
                        spread_radius: px(1.0),
                        inset: false,
                    }])
                });
        } else {
            el = el.on_mouse_down(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            });
        }

        if let Some(on_click) = self.on_click {
            let next_val = !self.checked;
            el = el.on_click(move |_, window, cx| {
                (on_click)(&next_val, window, cx);
            });
        }

        el
    }
}
