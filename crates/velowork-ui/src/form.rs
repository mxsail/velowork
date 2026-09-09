//! Unified Form and FormItem design system components.
//!
//! Provides a standardized layout for label + input controls across dialogs,
//! settings panels, and forms.
//!
//! Inspired by Ant Design's Form component, customized for GPUI + Rust.

use gpui::*;
use crate::design::semantic::SemanticPalette;
use crate::focus_group::{FocusGroup, FocusGroupExt};
use crate::icon::AppIcon;
use crate::theme::ThemeColors;
use crate::tokens::*;
use crate::tooltip::Tooltip;
use crate::{h_flex, v_flex};

/// Form layout orientation options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FormLayout {
    /// Label positioned vertically above input control (default for modal dialogs).
    #[default]
    Vertical,
    /// Label positioned horizontally to the left of input control.
    Horizontal,
    /// Compact single-line horizontal flow.
    Inline,
}

/// A unified FormItem builder representing a single form field (label + required star + control + error/help).
pub struct FormItem {
    id: ElementId,
    label: Option<SharedString>,
    required: bool,
    tooltip: Option<SharedString>,
    help: Option<SharedString>,
    error: Option<SharedString>,
    layout: FormLayout,
    label_width: Option<Pixels>,
    justify_between: bool,
    pub focus_handle: Option<FocusHandle>,
    child: Option<AnyElement>,
}

impl FormItem {
    /// Create a new FormItem with a unique element ID.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            label: None,
            required: false,
            tooltip: None,
            help: None,
            error: None,
            layout: FormLayout::Vertical,
            label_width: None,
            justify_between: false,
            focus_handle: None,
            child: None,
        }
    }

    /// Explicitly associate a FocusHandle with this form item for automatic Tab focus cycling.
    pub fn focus(mut self, handle: &FocusHandle) -> Self {
        self.focus_handle = Some(handle.clone());
        self
    }

    /// Optionally associate a FocusHandle with this form item.
    pub fn focus_opt(mut self, handle: Option<FocusHandle>) -> Self {
        self.focus_handle = handle;
        self
    }

    /// Set the field label text.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Display a red required asterisk '*' indicator next to the label.
    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    /// Display a help icon with a tooltip message next to the label.
    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// Display secondary help description text beneath the input control.
    pub fn help(mut self, help: impl Into<SharedString>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// Display a validation error message in red beneath the input control.
    pub fn error(mut self, error: impl Into<SharedString>) -> Self {
        self.error = Some(error.into());
        self
    }

    /// Optionally display a validation error message in red beneath the input control.
    pub fn error_opt(mut self, error: Option<impl Into<SharedString>>) -> Self {
        self.error = error.map(Into::into);
        self
    }

    /// Override the layout orientation for this specific item.
    pub fn layout(mut self, layout: FormLayout) -> Self {
        self.layout = layout;
        self
    }

    /// Set explicit fixed width for the label column (used in Horizontal layout).
    pub fn label_width(mut self, width: Pixels) -> Self {
        self.label_width = Some(width);
        self
    }

    /// Align label to left and control to right (ideal for toggle switch rows).
    pub fn justify_between(mut self, justify: bool) -> Self {
        self.justify_between = justify;
        self
    }

    /// Set the input control child element.
    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.child = Some(child.into_element().into_any_element());
        self
    }

    /// Render the FormItem into a GPUI `Stateful<Div>` element given theme and app context.
    pub fn render(mut self, t: &ThemeColors, cx: &App) -> Stateful<Div> {
        let p = SemanticPalette::from_theme(t);

        // 1. Build Label Node if label text is provided
        let label_el = self.label.as_ref().map(|label_text| {
            let mut label_row = h_flex().items_center().gap(SPACE_XS);

            // Required asterisk indicator '*'
            if self.required {
                label_row = label_row.child(
                    div()
                        .text_size(ui_text_md(cx))
                        .text_color(p.status_error)
                        .child("*"),
                );
            }

            // Label text with unified typography (ui_text_md) and color (text_secondary)
            let focus_handle = self.focus_handle.clone();
            let mut label_text_el = div()
                .text_size(ui_text_md(cx))
                .text_color(p.text_secondary)
                .child(label_text.clone());

            if let Some(fh) = focus_handle {
                label_text_el = label_text_el
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        window.focus(&fh, cx);
                    });
            }

            label_row = label_row.child(label_text_el);

            // Optional Info Tooltip icon
            if let Some(tip) = &self.tooltip {
                let tip_text = tip.clone();
                label_row = label_row.child(
                    div()
                        .id(ElementId::Name(format!("{}-tip", self.id).into()))
                        .cursor_pointer()
                        .child(
                            AppIcon::Help
                                .size(ICON_SM)
                                .text_color(p.text_muted),
                        )
                        .tooltip(move |_, cx| {
                            cx.new(|_| Tooltip::new(tip_text.clone())).into()
                        }),
                );
            }

            label_row
        });

        // 2. Build Secondary Message Node (Error text overrides Help text)
        let message_el = if let Some(err) = &self.error {
            Some(
                div()
                    .text_size(ui_text_xs(cx))
                    .text_color(p.status_error)
                    .child(err.clone()),
            )
        } else if let Some(help) = &self.help {
            Some(
                div()
                    .text_size(ui_text_xs(cx))
                    .text_color(p.text_muted)
                    .child(help.clone()),
            )
        } else {
            None
        };

        // 3. Assemble Layout based on FormLayout
        let item_id = self.id.clone();
        let child = self.child.take();
        match self.layout {
            FormLayout::Vertical => {
                let mut container = v_flex().id(item_id).w_full().gap(SPACE_XS);
                if let Some(lbl) = label_el {
                    container = container.child(lbl);
                }
                if let Some(c) = child {
                    container = container.child(c);
                }
                if let Some(msg) = message_el {
                    container = container.child(msg);
                }
                container
            }
            FormLayout::Horizontal => {
                let mut item_container = v_flex().id(item_id).w_full().gap(SPACE_XS);
                let mut row = h_flex().w_full().items_center().gap(SPACE_MD);

                if self.justify_between {
                    row = row.justify_between();
                    if let Some(lbl) = label_el {
                        row = row.child(div().flex_shrink_0().child(lbl));
                    }
                    if let Some(c) = child {
                        row = row.child(div().flex_shrink_0().child(c));
                    }
                } else {
                    if let Some(lbl) = label_el {
                        let mut lbl_wrapper = div().flex_shrink_0();
                        let lw = self.label_width.unwrap_or(px(140.0));
                        lbl_wrapper = lbl_wrapper.w(lw);
                        row = row.child(lbl_wrapper.child(lbl));
                    }

                    if let Some(c) = child {
                        row = row.child(div().flex_1().child(c));
                    }
                }

                item_container = item_container.child(row);
                if let Some(msg) = message_el {
                    let indent = if self.justify_between {
                        px(0.0)
                    } else {
                        self.label_width.unwrap_or(px(140.0)) + px(12.0)
                    };
                    item_container = item_container.child(div().pl(indent).child(msg));
                }

                item_container
            }
            FormLayout::Inline => {
                let mut row = h_flex().id(item_id).items_center().gap(SPACE_SM);
                if let Some(lbl) = label_el {
                    row = row.child(lbl);
                }
                if let Some(c) = child {
                    row = row.child(c);
                }
                row
            }
        }
    }
}

/// Convenience builder function for [`FormItem`].
pub fn form_item(id: impl Into<ElementId>) -> FormItem {
    FormItem::new(id)
}

/// Form container builder that manages layout configuration and automatic Tab focus cycling across child fields.
pub struct Form {
    id: ElementId,
    layout: FormLayout,
    gap: Pixels,
    label_width: Option<Pixels>,
    justify_between: bool,
    focus_group: FocusGroup,
    children: Vec<AnyElement>,
}

impl Form {
    /// Create a new Form container.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            layout: FormLayout::Vertical,
            gap: SPACE_MD,
            label_width: None,
            justify_between: false,
            focus_group: FocusGroup::new(),
            children: Vec::new(),
        }
    }

    /// Access the underlying [`FocusGroup`] for this Form.
    pub fn focus_group(&self) -> &FocusGroup {
        &self.focus_group
    }

    /// Add a [`FocusHandle`] into this Form's focus group for Tab cycling.
    pub fn add_focus(self, handle: &FocusHandle) -> Self {
        self.focus_group.add(handle.clone());
        self
    }

    /// Add multiple [`FocusHandle`]s into this Form's focus group for Tab cycling.
    pub fn extend_focus(self, handles: impl IntoIterator<Item = FocusHandle>) -> Self {
        self.focus_group.extend(handles);
        self
    }

    /// Set the default layout orientation for items in this Form.
    pub fn layout(mut self, layout: FormLayout) -> Self {
        self.layout = layout;
        self
    }

    /// Set the vertical/horizontal gap between items.
    pub fn gap(mut self, gap: Pixels) -> Self {
        self.gap = gap;
        self
    }

    /// Set default label column width for items in this Form.
    pub fn label_width(mut self, width: Pixels) -> Self {
        self.label_width = Some(width);
        self
    }

    /// Align label to left and control to right across all form items.
    pub fn justify_between(mut self, justify: bool) -> Self {
        self.justify_between = justify;
        self
    }

    /// Add a FormItem to the form. Inherits Form's layout, label width, and alignment if item does not override them.
    /// Also registers the item's `focus_handle` (if any) into the Form's `FocusGroup`.
    pub fn item(mut self, mut item: FormItem, t: &ThemeColors, cx: &App) -> Self {
        if item.layout == FormLayout::Vertical && self.layout != FormLayout::Vertical {
            item.layout = self.layout;
        }
        if item.label_width.is_none() && self.label_width.is_some() {
            item.label_width = self.label_width;
        }
        if !item.justify_between && self.justify_between {
            item.justify_between = self.justify_between;
        }
        if let Some(fh) = item.focus_handle.take() {
            self.focus_group.add(fh);
        }
        self.children.push(item.render(t, cx).into_any_element());
        self
    }

    /// Add any arbitrary element to the form.
    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_element().into_any_element());
        self
    }

    /// Add multiple elements to the form.
    pub fn children(mut self, children: impl IntoIterator<Item = impl IntoElement>) -> Self {
        self.children.extend(children.into_iter().map(|c| c.into_element().into_any_element()));
        self
    }

    /// Render the Form container with automatic Tab focus navigation attached.
    pub fn render(self) -> Stateful<Div> {
        let fg = self.focus_group;
        let el = match self.layout {
            FormLayout::Inline => {
                h_flex().id(self.id).items_center().gap(self.gap).children(self.children)
            }
            _ => {
                v_flex().id(self.id).w_full().gap(self.gap).children(self.children)
            }
        };
        el.tab_cycle(&fg)
    }
}

/// Convenience builder function for [`Form`].
pub fn form(id: impl Into<ElementId>) -> Form {
    Form::new(id)
}

#[cfg(test)]
mod tests {
    use super::{form, form_item, FormLayout};

    #[test]
    fn test_form_item_builder() {
        let item = form_item("test-field")
            .label("Username")
            .required(true)
            .tooltip("Your username")
            .help("Choose a unique handle")
            .layout(FormLayout::Vertical);

        assert_eq!(item.label, Some("Username".into()));
        assert!(item.required);
        assert_eq!(item.tooltip, Some("Your username".into()));
        assert_eq!(item.help, Some("Choose a unique handle".into()));
    }

    #[test]
    fn test_form_builder() {
        let _item1 = form_item("f1");
        let _item2 = form_item("f2");

        let my_form = form("my-form");
        assert_eq!(my_form.id, gpui::ElementId::Name("my-form".into()));
    }
}
