//! Native Checkbox component for Velowork.

use std::rc::Rc;
use crate::design::semantic::SemanticPalette;
use crate::icon::AppIcon;
use crate::input::focus_ring_shadows;
use crate::theme::theme;
use crate::tokens::{ICON_SM, RADIUS_SM, SPACE_SM, ui_text_md};
use gpui::prelude::FluentBuilder;
use gpui::*;

#[derive(IntoElement)]
pub struct Checkbox {
    id: ElementId,
    checked: bool,
    disabled: bool,
    focused: bool,
    label: Option<SharedString>,
    focus_handle: Option<FocusHandle>,
    on_click: Option<Rc<dyn Fn(&bool, &mut Window, &mut App) + 'static>>,
}

impl Checkbox {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            checked: false,
            disabled: false,
            focused: false,
            label: None,
            focus_handle: None,
            on_click: None,
        }
    }

    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    pub fn focus(mut self, handle: &FocusHandle) -> Self {
        self.focus_handle = Some(handle.clone());
        self
    }

    pub fn focus_opt(mut self, handle: Option<&FocusHandle>) -> Self {
        self.focus_handle = handle.cloned();
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

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
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

impl RenderOnce for Checkbox {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_context(cx);
        let is_focused = self.focused || self.focus_handle.as_ref().is_some_and(|fh| fh.is_focused(window));

        let box_bg = if self.checked {
            p.surface_accent
        } else {
            p.surface_card
        };

        let box_border = if is_focused || self.checked {
            p.border_active
        } else {
            p.border_subtle
        };

        let check_icon = if self.checked {
            Some(
                AppIcon::Check
                    .size(ICON_SM)
                    .text_color(p.text_on_accent),
            )
        } else {
            None
        };

        let group_id: SharedString = format!("checkbox-grp-{}", self.id).into();

        let box_el = div()
            .size(px(16.0))
            .flex_shrink_0()
            .rounded(RADIUS_SM)
            .border_1()
            .border_color(box_border)
            .bg(box_bg)
            .flex()
            .items_center()
            .justify_center()
            .when(is_focused && !self.disabled, |b| {
                b.shadow(focus_ring_shadows(&t))
            })
            .when(!self.checked && !self.disabled && !is_focused, |b| {
                b.group_hover(group_id.clone(), |s| {
                    s.border_color(p.surface_accent.opacity(0.6)).bg(p.surface_hover)
                })
            })
            .children(check_icon);

        let mut row = div()
            .id(self.id)
            .group(group_id)
            .flex()
            .items_center()
            .gap(SPACE_SM)
            .when(!self.disabled, |r| r.cursor_pointer())
            .when(self.disabled, |r| r.cursor_not_allowed().opacity(0.5))
            .child(box_el);

        if let Some(fh) = self.focus_handle.as_ref() {
            row = row.track_focus(fh);
        }

        if let Some(label) = self.label {
            row = row.child(
                div()
                    .text_size(ui_text_md(cx))
                    .text_color(if self.disabled {
                        p.text_muted
                    } else {
                        p.text_primary
                    })
                    .child(label),
            );
        }

        if let Some(on_click) = self.on_click {
            let next_val = !self.checked;
            row = row.on_click(move |_, window, cx| {
                (on_click)(&next_val, window, cx);
            });
        }

        row
    }
}
