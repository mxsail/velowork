//! Custom Ant Design inspired reusable Button component.

use std::sync::Arc;
use crate::icon::AppIcon;
use crate::theme::ThemeColors;
use crate::tooltip::Tooltip;
use crate::behavior::{ActiveFeedbackBehavior, HoverBehavior, StatefulElementBehaviorExt};
use crate::design::appearance::{ControlAppearance, ControlSize, ControlVariant};
use crate::design::density::UiDensity;
use crate::design::semantic::SemanticPalette;
use gpui::*;

/// Flexible icon representation for button.
pub enum ButtonIcon {
    /// Catalog icon (e.g., `AppIcon::Plus`).
    Icon(AppIcon),
    /// Pre-rendered element closure.
    Element(Arc<dyn Fn() -> AnyElement + Send + Sync>),
}

impl From<AppIcon> for ButtonIcon {
    #[inline(always)]
    fn from(icon: AppIcon) -> Self {
        Self::Icon(icon)
    }
}

impl From<&str> for ButtonIcon {
    fn from(s: &str) -> Self {
        Self::Icon(AppIcon::from_str(s).unwrap_or_else(AppIcon::default))
    }
}

impl From<String> for ButtonIcon {
    fn from(s: String) -> Self {
        Self::Icon(AppIcon::from_str(&s).unwrap_or_else(AppIcon::default))
    }
}

impl From<SharedString> for ButtonIcon {
    fn from(s: SharedString) -> Self {
        Self::Icon(AppIcon::from_str(&s).unwrap_or_else(AppIcon::default))
    }
}

/// Reusable Ant Design inspired Button component.
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    theme: ThemeColors,
    variant: ControlVariant,
    size: ControlSize,
    density: Option<UiDensity>,
    ui_scale: Option<f32>,
    label: Option<SharedString>,
    danger: bool,
    selected: bool,
    loading: bool,
    disabled: bool,
    full_width: bool,
    custom_px: Option<Pixels>,
    custom_py: Option<Pixels>,
    custom_opacity: Option<f32>,
    custom_text_size: Option<Pixels>,
    icon_left: Option<ButtonIcon>,
    icon_right: Option<ButtonIcon>,
    tooltip: Option<SharedString>,
    focus_handle: Option<FocusHandle>,
    on_click: Option<Arc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
}


impl Button {
    /// Create a new Button instance.
    pub fn new(id: impl Into<ElementId>, t: &ThemeColors) -> Self {
        Self {
            id: id.into(),
            theme: *t,
            variant: ControlVariant::Secondary,
            size: ControlSize::Default,
            density: None,
            ui_scale: None,
            label: None,
            danger: false,
            selected: false,
            loading: false,
            disabled: false,
            full_width: false,
            custom_px: None,
            custom_py: None,
            custom_opacity: None,
            custom_text_size: None,
            icon_left: None,
            icon_right: None,
            tooltip: None,
            focus_handle: None,
            on_click: None,
        }
    }

    /// Attach a FocusHandle so the button is keyboard-focusable (via Tab cycle),
    /// shows the active focus ring when focused, and responds to Enter/Space.
    pub fn focus_handle(mut self, focus_handle: &FocusHandle) -> Self {
        self.focus_handle = Some(focus_handle.clone());
        self
    }

    /// Set whether the button is selected/focused.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Set the button text label.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Set the button variant from the canonical design-system variant.
    pub fn variant(mut self, variant: ControlVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Shortcut for Primary variant.
    pub fn primary(self) -> Self {
        self.variant(ControlVariant::Primary)
    }

    /// Shortcut for Default (Secondary) variant.
    pub fn default(self) -> Self {
        self.variant(ControlVariant::Secondary)
    }

    /// Shortcut for Dashed (Outline) variant.
    pub fn dashed(self) -> Self {
        self.variant(ControlVariant::Outline)
    }

    /// Shortcut for Text (Ghost) variant.
    pub fn text(self) -> Self {
        self.variant(ControlVariant::Ghost)
    }

    /// Shortcut for Link variant.
    pub fn link(self) -> Self {
        self.variant(ControlVariant::Link)
    }

    /// Set the button size from the canonical design-system size tier.
    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }

    /// Set the UI density level (Compact / Default / Comfortable).
    pub fn density(mut self, density: UiDensity) -> Self {
        self.density = Some(density);
        self
    }

    /// Set the UI scale factor (1.0 = 100%).
    pub fn ui_scale(mut self, scale: f32) -> Self {
        self.ui_scale = Some(scale);
        self
    }

    /// Shortcut for Large size (40px).
    pub fn large(self) -> Self {
        self.size(ControlSize::Large)
    }

    /// Shortcut for Middle size (32px, default).
    pub fn middle(self) -> Self {
        self.size(ControlSize::Default)
    }

    /// Shortcut for Small size (24px).
    pub fn small(self) -> Self {
        self.size(ControlSize::Compact)
    }

    /// Set whether the button is in danger state (red style).
    pub fn danger(mut self, danger: bool) -> Self {
        self.danger = danger;
        self
    }

    /// Set whether the button is in loading state.
    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    /// Set whether the button is disabled.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Set whether the button expands to full width.
    pub fn full_width(mut self, full_width: bool) -> Self {
        self.full_width = full_width;
        self
    }

    /// Shortcut to set full width.
    pub fn w_full(self) -> Self {
        self.full_width(true)
    }

    /// Override horizontal padding.
    pub fn px(mut self, px: impl Into<Pixels>) -> Self {
        self.custom_px = Some(px.into());
        self
    }

    /// Override vertical padding.
    pub fn py(mut self, py: impl Into<Pixels>) -> Self {
        self.custom_py = Some(py.into());
        self
    }

    /// Override opacity.
    pub fn opacity(mut self, opacity: impl Into<f32>) -> Self {
        self.custom_opacity = Some(opacity.into());
        self
    }

    /// Override text size.
    pub fn text_size(mut self, size: impl Into<Pixels>) -> Self {
        self.custom_text_size = Some(size.into());
        self
    }

    /// Set cursor style (compatible with Div builder chain).
    pub fn cursor(self, _cursor: CursorStyle) -> Self {
        self
    }

    /// Set icon displayed before text/label.
    pub fn icon_left(mut self, icon: impl Into<ButtonIcon>) -> Self {
        self.icon_left = Some(icon.into());
        self
    }

    /// Set icon displayed after text/label.
    pub fn icon_right(mut self, icon: impl Into<ButtonIcon>) -> Self {
        self.icon_right = Some(icon.into());
        self
    }

    /// Set tooltip displayed on hover (with Chinese tooltip support).
    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// Attach a click event handler.
    pub fn on_click<F>(mut self, listener: F) -> Self
    where
        F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    {
        self.on_click = Some(Arc::new(listener));
        self
    }

    // Helper getters for testing
    #[cfg(test)]
    pub fn get_variant(&self) -> ControlVariant {
        self.variant
    }

    #[cfg(test)]
    pub fn get_size(&self) -> ControlSize {
        self.size
    }

    #[cfg(test)]
    pub fn is_danger(&self) -> bool {
        self.danger
    }

    #[cfg(test)]
    pub fn is_loading(&self) -> bool {
        self.loading
    }

    #[cfg(test)]
    pub fn is_disabled(&self) -> bool {
        self.disabled
    }
}

impl RenderOnce for Button {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let _t = self.theme;
        let element_id = self.id.clone();
        let is_interactive = !self.disabled && !self.loading;
        let is_focused = self
            .focus_handle
            .as_ref()
            .map(|fh| fh.is_focused(window))
            .unwrap_or(false);
        let is_active_or_selected = (self.selected || is_focused) && !self.disabled;

        // Size parameters — resolved from the frozen design system
        // (ControlAppearance) using the canonical `ControlSize` tier, so there
        // are no hardcoded size tuples left and the visual weight is shared
        // with every other control.
        let p = SemanticPalette::from_context(cx);
        let density = self.density.unwrap_or_else(|| crate::tokens::get_ui_density(cx));
        let ui_scale = self.ui_scale.unwrap_or_else(|| crate::tokens::ui_text_scale(cx));
        let geom = ControlAppearance::resolve(
            self.size,
            self.variant,
            &p,
            density,
            ui_scale,
        );
        let px_pad = self.custom_px.unwrap_or(geom.padding_x);
        let py_pad = self.custom_py.unwrap_or(geom.padding_y);
        let f_size = self.custom_text_size.unwrap_or(geom.font_size);

        // Resolve appearance from the frozen design system. Geometry is already
        // derived from `ControlAppearance`; colors come from the `SemanticPalette`
        // it wraps. Hover is background-only (see `HoverBehavior`), matching the
        // prototype's `--hover-*` treatment and the other unified controls.
        let transparent = gpui::hsla(0.0, 0.0, 0.0, 0.0);

        let mut final_bg = geom.bg;
        let mut final_border = geom.border_color;
        let mut final_fg = geom.text_color;
        let mut hover_bg = geom.bg_hover;

        if self.danger {
            match self.variant {
                ControlVariant::Primary | ControlVariant::Danger => {
                    final_bg = p.surface_danger;
                    final_border = transparent;
                    final_fg = p.text_on_accent;
                    hover_bg = p.surface_danger;
                }
                _ => {
                    // Secondary / Outline / Ghost / Link + danger → red text on the
                    // base background with a danger hover overlay (prototype `--danger`).
                    final_border = p.surface_danger;
                    final_fg = p.surface_danger;
                    hover_bg = Hsla { a: 0.12, ..p.surface_danger };
                }
            }
        }

        // Disabled / loading adjustments.
        let computed_opacity = if self.disabled {
            final_fg = p.text_muted;
            final_bg = match self.variant {
                ControlVariant::Primary => Hsla { a: 0.6, ..p.surface_raised },
                ControlVariant::Secondary | ControlVariant::Outline => p.surface_raised,
                ControlVariant::Ghost | ControlVariant::Link | ControlVariant::Danger => transparent,
            };
            final_border = match self.variant {
                ControlVariant::Secondary | ControlVariant::Outline => p.border_subtle,
                _ => transparent,
            };
            hover_bg = transparent;
            0.65
        } else if self.loading {
            0.50
        } else {
            1.0
        };

        let final_opacity = self.custom_opacity.unwrap_or(computed_opacity);

        if is_active_or_selected {
            if self.danger {
                final_border = p.surface_danger;
            } else {
                final_border = p.border_active;
                if self.variant == ControlVariant::Secondary {
                    final_bg = p.surface_raised;
                }
            }
        }

        let mut container = div()
            .id(self.id)
            .h(geom.height)
            .min_h(geom.height)
            .max_h(geom.height)
            .px(px_pad)
            .py(py_pad)
            .rounded(geom.radius)
            .flex()
            .items_center()
            .justify_center()
            .gap(geom.gap)
            .opacity(final_opacity)
            .text_size(f_size)
            .font_weight(geom.font_weight)
            .line_height(geom.line_height)
            .text_color(final_fg);

        if self.full_width {
            container = container.w_full();
        } else {
            container = container.flex_shrink_0();
        }

        if let Some(ref fh) = self.focus_handle {
            container = container.track_focus(fh);
        }

        container = container.border_1().border_color(final_border);
        container = container.bg(final_bg);

        if is_active_or_selected {
            let ring_color = if self.danger {
                Hsla { a: 0.35, ..p.surface_danger }
            } else {
                Hsla { a: 0.35, ..p.border_active }
            };
            container = container.shadow(vec![BoxShadow {
                color: ring_color,
                offset: point(px(0.0), px(0.0)),
                blur_radius: px(3.0),
                spread_radius: px(1.5),
                inset: false,
            }]);
        } else if !self.disabled && matches!(self.variant, ControlVariant::Primary | ControlVariant::Danger) {
            container = container.shadow(vec![BoxShadow {
                color: gpui::rgba(0x00000028).into(),
                offset: point(px(0.0), px(1.0)),
                blur_radius: px(2.0),
                spread_radius: px(0.0),
                inset: false,
            }]);
        }

        // Render Left Icon / Loading Spinner (using LoaderCircle with smooth 360-degree rotation animation)
        if self.loading {
            let anim_id = format!("{:?}-spinner", element_id);
            let spinner = crate::spinner::loading_spinner(anim_id, geom.icon_size, final_fg);
            container = container.child(spinner);
        } else if let Some(icon) = self.icon_left {
            match icon {
                ButtonIcon::Icon(icon) => {
                    let icon_elem = icon.size(geom.icon_size).text_color(final_fg);
                    container = container.child(icon_elem);
                }
                ButtonIcon::Element(render) => {
                    container = container.child((render)());
                }
            }
        }

        // Render Label Text
        if let Some(label) = self.label {
            container = container.child(label);
        }

        // Render Right Icon (if not loading)
        if !self.loading {
            if let Some(icon) = self.icon_right {
                match icon {
                    ButtonIcon::Icon(icon) => {
                        let icon_elem = icon.size(geom.icon_size).text_color(final_fg);
                        container = container.child(icon_elem);
                    }
                    ButtonIcon::Element(render) => {
                        container = container.child((render)());
                    }
                }
            }
        }

        if is_interactive {
            container = container.cursor_pointer();
            container = container.stateful_behavior(HoverBehavior {
                hover_bg: if self.variant == ControlVariant::Link {
                    transparent
                } else {
                    hover_bg
                },
                hover_fg: if self.variant == ControlVariant::Link {
                    Some(p.surface_accent_hover)
                } else {
                    None
                },
                underline: self.variant == ControlVariant::Link,
                ..Default::default()
            });
            container = container.stateful_behavior(ActiveFeedbackBehavior::default());

            if let Some(ref fh) = self.focus_handle {
                let fh_clone = fh.clone();
                container = container.on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    window.focus(&fh_clone, cx);
                });
            }

            if let Some(on_click) = self.on_click {
                container = container.on_click(move |ev, window, cx| on_click(ev, window, cx));
            }
        } else {
            container = container.cursor_not_allowed();
        }

        if let Some(ref tip) = self.tooltip {
            let tip_text = tip.clone();
            container = container.tooltip(move |_, cx| { let __tip = tip_text.clone(); cx.new(|_| Tooltip::new(__tip)).into() });
        }

        container
    }
}

/// Standard secondary button helper function.
pub fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    t: &ThemeColors,
) -> Button {
    Button::new(id, t).label(label).default()
}

/// Primary action button helper function (e.g., "Add", "Create", "Save").
pub fn button_primary(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    t: &ThemeColors,
) -> Button {
    Button::new(id, t).label(label).primary()
}

#[cfg(test)]
mod tests {
    use super::{Button, ControlSize, ControlVariant};
    use crate::theme::DARK_THEME;

    #[test]
    fn test_button_builder_options() {
        let t = DARK_THEME;
        let btn = Button::new("test-btn", &t)
            .label("Submit")
            .primary()
            .large()
            .danger(true)
            .loading(true)
            .disabled(false);

        assert_eq!(btn.get_variant(), ControlVariant::Primary);
        assert_eq!(btn.get_size(), ControlSize::Large);
        assert!(btn.is_danger());
        assert!(btn.is_loading());
        assert!(!btn.is_disabled());
    }

    #[test]
    fn test_button_types() {
        let t = DARK_THEME;
        let btn_def = Button::new("b1", &t).default();
        let btn_dash = Button::new("b2", &t).dashed();
        let btn_txt = Button::new("b3", &t).text();
        let btn_link = Button::new("b4", &t).link();

        assert_eq!(btn_def.get_variant(), ControlVariant::Secondary);
        assert_eq!(btn_dash.get_variant(), ControlVariant::Outline);
        assert_eq!(btn_txt.get_variant(), ControlVariant::Ghost);
        assert_eq!(btn_link.get_variant(), ControlVariant::Link);
    }
}
