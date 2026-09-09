//! Color Picker component — standalone overlay entity in velowork-ui.
//!
//! Provides a feature-rich, accessible color picker with:
//! - Curated preset palette matrix for quick selection
//! - Real-time `#RRGGBB` hex text input with live validation
//! - Live color swatch preview
//! - Backdrop dismissal on click outside
//! - Keyboard navigation (Escape/Cancel to close, Enter to confirm)

use crate::Cancel;
use crate::overlay::CloseEvent;
use crate::theme::theme;
use crate::tokens::{ui_text_sm, RADIUS_STD, SPACE_MD, SPACE_SM, SPACE_XS};
use crate::simple_input::{InputChangedEvent, InputEvent, SimpleInput, SimpleInputState};
use crate::button::Button;
use crate::h_flex;
use velowork_core::theme::{hex_to_u32, u32_to_hex};
use velowork_i18n::i18n;
use gpui::prelude::*;
use gpui::*;

/// Curated preset colors for quick selection (30 distinct colors across 5 hues)
pub const PRESET_COLORS: &[u32] = &[
    // Grayscale & Neutrals
    0x000000, 0x1E1E1E, 0x2D2D2D, 0x505050, 0x808080, 0xFFFFFF,
    // Reds & Oranges
    0xF44747, 0xE06C75, 0xFF5555, 0xD94F4F, 0xD19A66, 0xCB4B16,
    // Yellows & Greens
    0xD7BA7D, 0xE5C07B, 0xF1FA8C, 0x6A9955, 0x98C379, 0x50FA7B,
    // Cyans & Blues
    0x4EC9B0, 0x56B6C2, 0x8BE9FD, 0x569CD6, 0x61AFEF, 0x268BD2,
    // Purples & Pinks
    0xC586C0, 0xC678DD, 0xBD93F9, 0xAE81FF, 0xFF79C6, 0xE06C9F,
];

/// Events emitted by ColorPicker.
#[derive(Clone, Debug)]
pub enum ColorPickerEvent {
    Close,
    ColorSelected(u32),
}

impl CloseEvent for ColorPickerEvent {
    fn is_close(&self) -> bool {
        matches!(self, Self::Close)
    }
}

impl EventEmitter<ColorPickerEvent> for ColorPicker {}

/// Standalone color picker popover entity.
pub struct ColorPicker {
    current_color: u32,
    position: Point<Pixels>,
    input_state: Entity<SimpleInputState>,
    is_valid_hex: bool,
}

impl ColorPicker {
    pub fn new(
        initial_color: u32,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> Self {
        let hex_str = u32_to_hex(initial_color);

        let input_state = cx.new(|cx| {
            SimpleInputState::new(cx)
                .placeholder("#RRGGBB")
                .default_value(&hex_str)
        });

        cx.subscribe(&input_state, |this, _, _event: &InputChangedEvent, cx| {
            let val = this.input_state.read(cx).value().trim().to_string();
            if let Some(parsed) = hex_to_u32(&val) {
                this.current_color = parsed;
                this.is_valid_hex = true;
            } else {
                this.is_valid_hex = false;
            }
            cx.notify();
        }).detach();

        cx.subscribe(&input_state, |this, _, event: &InputEvent, cx| {
            if *event == InputEvent::PressEnter {
                if this.is_valid_hex {
                    this.confirm(cx);
                }
            }
        }).detach();

        Self {
            current_color: initial_color,
            position,
            input_state,
            is_valid_hex: true,
        }
    }

    pub fn current_color(&self) -> u32 {
        self.current_color
    }

    fn select_preset(&mut self, color: u32, cx: &mut Context<Self>) {
        self.current_color = color;
        self.is_valid_hex = true;
        let hex = u32_to_hex(color);
        self.input_state.update(cx, |input, cx| {
            input.set_value(hex, cx);
        });
        cx.notify();
    }

    fn confirm(&mut self, cx: &mut Context<Self>) {
        let color = self.current_color;
        cx.emit(ColorPickerEvent::ColorSelected(color));
    }

    fn close(&self, cx: &mut Context<Self>) {
        cx.emit(ColorPickerEvent::Close);
    }
}

impl Render for ColorPicker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let current_color = self.current_color;
        let is_valid = self.is_valid_hex;
        let input_state = self.input_state.clone();
        let position = self.position;

        let panel = crate::popover::popover_panel("color-picker-popover", &t)
            .w(px(230.0))
            .p(SPACE_MD)
            .flex()
            .flex_col()
            .gap(SPACE_MD)
            .on_mouse_down(MouseButton::Right, |_, _, cx| {
                cx.stop_propagation();
            })
            // 1. Preset colors grid (6 columns x 5 rows)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(SPACE_XS)
                    .child(
                        div()
                            .text_size(ui_text_sm(cx))
                            .text_color(rgb(t.text_secondary))
                            .child(i18n!(cx, "dialog.color_picker.presets")),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(px(5.0))
                            .children(PRESET_COLORS.iter().map(|&color| {
                                let is_selected = color == current_color;
                                div()
                                    .id(ElementId::Name(format!("preset-{:06x}", color).into()))
                                    .w(px(28.0))
                                    .h(px(22.0))
                                    .rounded(RADIUS_STD)
                                    .bg(rgb(color))
                                    .cursor_pointer()
                                    .border_1()
                                    .border_color(if is_selected {
                                        rgb(t.border_active)
                                    } else {
                                        rgb(t.border)
                                    })
                                    .when(is_selected, |d| {
                                        d.shadow_sm()
                                    })
                                    .hover(|s| s.opacity(0.85))
                                    .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, _window, cx| {
                                        cx.stop_propagation();
                                        this.select_preset(color, cx);
                                    }))
                            })),
                    ),
            )
            // 2. Hex input + live swatch row
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(SPACE_XS)
                    .child(
                        div()
                            .text_size(ui_text_sm(cx))
                            .text_color(rgb(t.text_secondary))
                            .child(i18n!(cx, "dialog.color_picker.hex_code")),
                    )
                    .child(
                        h_flex()
                            .gap(SPACE_SM)
                            .items_center()
                            // Current color swatch preview
                            .child(
                                div()
                                    .w(px(32.0))
                                    .h(px(28.0))
                                    .rounded(RADIUS_STD)
                                    .bg(rgb(current_color))
                                    .border_1()
                                    .border_color(rgb(t.border)),
                            )
                            // Hex text input
                            .child(
                                div()
                                    .flex_1()
                                    .child(SimpleInput::new(&input_state)),
                            ),
                    ),
            )
            // 3. Actions (Confirm / Cancel)
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(
                        Button::new("color-picker-cancel", &t)
                            .label(i18n!(cx, "common.cancel"))
                            .small()
                            .text()
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.close(cx);
                            })),
                    )
                    .child(
                        Button::new("color-picker-confirm", &t)
                            .label(i18n!(cx, "common.confirm"))
                            .small()
                            .primary()
                            .disabled(!is_valid)
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.confirm(cx);
                            })),
                    ),
            );

        div()
            .key_context("ColorPicker")
            .on_action(cx.listener(|this, _: &Cancel, _window, cx| {
                this.close(cx);
            }))
            .absolute()
            .inset_0()
            .occlude()
            .id("color-picker-backdrop")
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _window, cx| {
                this.close(cx);
            }))
            .on_mouse_down(MouseButton::Right, cx.listener(|this, _, _window, cx| {
                this.close(cx);
            }))
            .on_scroll_wheel(|_, _, cx| {
                cx.stop_propagation();
            })
            .child(deferred(
                anchored()
                    .position(position)
                    .snap_to_window()
                    .child(panel),
            ))
    }
}
