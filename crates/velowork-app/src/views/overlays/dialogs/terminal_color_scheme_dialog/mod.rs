//! Terminal Color Scheme Manager Dialog ("终端配色方案管理").
//!
//! Provides a full-featured modal dialog for inspecting built-in ANSI palettes,
//! creating/editing/duplicating/deleting custom color schemes, viewing real-time
//! ANSI terminal preview rendering, and visual 20-color swatch adjustment.

use crate::keybindings::Cancel;
use crate::settings::settings_entity;
use crate::theme::theme;
use crate::views::components::{modal_content, modal_header};
use gpui::prelude::*;
use gpui::*;
use velowork_core::theme::{
    BUILTIN_COLOR_SCHEMES, CustomTerminalColorScheme, DARK_PALETTE, TerminalPalette,
    get_terminal_palette, is_built_in_color_scheme,
};
use velowork_i18n::i18n;
use velowork_ui::button::Button;
use velowork_ui::color_picker::{ColorPicker, ColorPickerEvent};
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::h_flex;
use velowork_ui::icon::AppIcon;
use velowork_ui::overlay::CloseEvent;
use velowork_ui::simple_input::{SimpleInput, SimpleInputState};
use velowork_ui::theme::with_alpha;
use velowork_ui::tokens::{
    RADIUS_LG, RADIUS_STD, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL, SPACE_XS,
    ui_text_md, ui_text_sm, ui_text_xs,
};
use velowork_ui::tooltip::Tooltip;

/// Events emitted by TerminalColorSchemeDialog.
#[derive(Clone, Debug)]
pub enum TerminalColorSchemeDialogEvent {
    Close,
    SchemeSaved,
}

impl CloseEvent for TerminalColorSchemeDialogEvent {
    fn is_close(&self) -> bool {
        matches!(self, Self::Close)
    }
}

impl EventEmitter<TerminalColorSchemeDialogEvent> for TerminalColorSchemeDialog {}

pub struct TerminalColorSchemeDialog {
    focus_handle: FocusHandle,
    /// Currently selected scheme ID or built-in name
    selected_scheme_id: String,
    /// Is currently selected scheme a built-in one?
    is_builtin: bool,
    /// Editable draft name for custom scheme
    draft_name: Entity<SimpleInputState>,
    /// Working copy of the palette being viewed/edited
    draft_palette: TerminalPalette,
    /// List of custom schemes loaded from settings
    custom_schemes: Vec<CustomTerminalColorScheme>,
    /// Global active color scheme name from settings
    active_scheme_name: String,
    /// Active color picker popover instance
    active_picker: Option<Entity<ColorPicker>>,
    /// Key of color currently being edited in picker (e.g. "foreground", "red")
    editing_color_key: Option<String>,
}

impl TerminalColorSchemeDialog {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let s = settings_entity(cx).read(cx).settings.clone();
        let active_scheme_name = s.color_scheme.clone();
        let custom_schemes = s.custom_terminal_color_schemes.clone();

        // Default to active scheme if valid, else first built-in
        let initial_id = if is_built_in_color_scheme(&active_scheme_name)
            || custom_schemes
                .iter()
                .any(|c| c.id == active_scheme_name || c.name == active_scheme_name)
        {
            active_scheme_name.clone()
        } else {
            "Dark".to_string()
        };

        let is_builtin = is_built_in_color_scheme(&initial_id);
        let draft_palette = if is_builtin {
            get_terminal_palette(&initial_id)
        } else if let Some(c) = custom_schemes
            .iter()
            .find(|c| c.id == initial_id || c.name == initial_id)
        {
            c.palette
        } else {
            DARK_PALETTE
        };

        let initial_name = if is_builtin {
            initial_id.clone()
        } else if let Some(c) = custom_schemes
            .iter()
            .find(|c| c.id == initial_id || c.name == initial_id)
        {
            c.name.clone()
        } else {
            "Custom Scheme".to_string()
        };

        let draft_name = cx.new(|cx| {
            SimpleInputState::new(cx)
                .placeholder(i18n!(cx, "terminal_color_schemes.name_placeholder"))
                .default_value(&initial_name)
                .read_only(is_builtin)
        });

        Self {
            focus_handle,
            selected_scheme_id: initial_id,
            is_builtin,
            draft_name,
            draft_palette,
            custom_schemes,
            active_scheme_name,
            active_picker: None,
            editing_color_key: None,
        }
    }

    fn select_scheme(&mut self, id_or_name: &str, cx: &mut Context<Self>) {
        self.selected_scheme_id = id_or_name.to_string();
        self.is_builtin = is_built_in_color_scheme(id_or_name);
        self.active_picker = None;
        self.editing_color_key = None;

        if self.is_builtin {
            self.draft_palette = get_terminal_palette(id_or_name);
            let name = id_or_name.to_string();
            self.draft_name.update(cx, |input, cx| {
                input.set_value(name, cx);
                input.set_read_only(true);
            });
        } else if let Some(custom) = self
            .custom_schemes
            .iter()
            .find(|c| c.id == id_or_name || c.name == id_or_name)
        {
            self.draft_palette = custom.palette;
            let name = custom.name.clone();
            self.draft_name.update(cx, |input, cx| {
                input.set_value(name, cx);
                input.set_read_only(false);
            });
        }
        cx.notify();
    }

    fn new_custom_scheme(&mut self, cx: &mut Context<Self>) {
        let count = self.custom_schemes.len() + 1;
        let new_id = format!(
            "custom_scheme_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        );
        let new_name = format!(
            "{} {}",
            i18n!(cx, "terminal_color_schemes.custom_prefix"),
            count
        );
        let new_palette = self.draft_palette;

        let new_scheme =
            CustomTerminalColorScheme::new(new_id.clone(), new_name.clone(), new_palette);
        self.custom_schemes.push(new_scheme);
        self.selected_scheme_id = new_id;
        self.is_builtin = false;
        self.active_picker = None;
        self.editing_color_key = None;

        self.draft_name.update(cx, |input, cx| {
            input.set_value(new_name, cx);
            input.set_read_only(false);
        });
        cx.notify();
    }

    fn duplicate_current_scheme(&mut self, cx: &mut Context<Self>) {
        let base_name = self.draft_name.read(cx).value().trim().to_string();
        let new_id = format!(
            "custom_scheme_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        );
        let new_name = format!(
            "{} (Copy)",
            if base_name.is_empty() {
                "Scheme"
            } else {
                &base_name
            }
        );
        let new_palette = self.draft_palette;

        let new_scheme =
            CustomTerminalColorScheme::new(new_id.clone(), new_name.clone(), new_palette);
        self.custom_schemes.push(new_scheme);
        self.selected_scheme_id = new_id;
        self.is_builtin = false;
        self.active_picker = None;
        self.editing_color_key = None;

        self.draft_name.update(cx, |input, cx| {
            input.set_value(new_name, cx);
            input.set_read_only(false);
        });
        cx.notify();
    }

    fn delete_custom_scheme(&mut self, scheme_id: &str, cx: &mut Context<Self>) {
        self.custom_schemes.retain(|c| c.id != scheme_id);
        if self.selected_scheme_id == scheme_id {
            self.select_scheme("Dark", cx);
        } else {
            cx.notify();
        }
    }

    fn open_color_picker(
        &mut self,
        key: &str,
        current_color: u32,
        pos: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if self.is_builtin {
            return;
        }
        self.editing_color_key = Some(key.to_string());
        let key_owned = key.to_string();

        let picker = cx.new(|cx| ColorPicker::new(current_color, pos, cx));
        cx.subscribe(
            &picker,
            move |this, _, event: &ColorPickerEvent, cx| match event {
                ColorPickerEvent::Close => {
                    this.active_picker = None;
                    this.editing_color_key = None;
                    cx.notify();
                }
                ColorPickerEvent::ColorSelected(new_color) => {
                    this.apply_color_change(&key_owned, *new_color, cx);
                    this.active_picker = None;
                    this.editing_color_key = None;
                    cx.notify();
                }
            },
        )
        .detach();

        self.active_picker = Some(picker);
        cx.notify();
    }

    fn apply_color_change(&mut self, key: &str, color: u32, cx: &mut Context<Self>) {
        if self.is_builtin {
            return;
        }
        match key {
            "foreground" => self.draft_palette.foreground = color,
            "background" => self.draft_palette.background = color,
            "cursor" => self.draft_palette.cursor = Some(color),
            "selection" => self.draft_palette.selection = Some(color),
            "black" => self.draft_palette.black = color,
            "red" => self.draft_palette.red = color,
            "green" => self.draft_palette.green = color,
            "yellow" => self.draft_palette.yellow = color,
            "blue" => self.draft_palette.blue = color,
            "magenta" => self.draft_palette.magenta = color,
            "cyan" => self.draft_palette.cyan = color,
            "white" => self.draft_palette.white = color,
            "bright_black" => self.draft_palette.bright_black = color,
            "bright_red" => self.draft_palette.bright_red = color,
            "bright_green" => self.draft_palette.bright_green = color,
            "bright_yellow" => self.draft_palette.bright_yellow = color,
            "bright_blue" => self.draft_palette.bright_blue = color,
            "bright_magenta" => self.draft_palette.bright_magenta = color,
            "bright_cyan" => self.draft_palette.bright_cyan = color,
            "bright_white" => self.draft_palette.bright_white = color,
            _ => {}
        }

        if let Some(custom) = self
            .custom_schemes
            .iter_mut()
            .find(|c| c.id == self.selected_scheme_id)
        {
            custom.palette = self.draft_palette;
        }
        cx.notify();
    }

    fn save_and_apply(&mut self, set_active: bool, cx: &mut Context<Self>) {
        // If current editing scheme is custom, save its name & palette into self.custom_schemes
        if !self.is_builtin {
            let name = self.draft_name.read(cx).value().trim().to_string();
            let final_name = if name.is_empty() {
                i18n!(cx, "terminal_color_schemes.unnamed_scheme")
            } else {
                name
            };

            if let Some(custom) = self
                .custom_schemes
                .iter_mut()
                .find(|c| c.id == self.selected_scheme_id)
            {
                custom.name = final_name;
                custom.palette = self.draft_palette;
            } else {
                self.custom_schemes.push(CustomTerminalColorScheme::new(
                    self.selected_scheme_id.clone(),
                    final_name,
                    self.draft_palette,
                ));
            }
        }

        let custom_list = self.custom_schemes.clone();
        let target_active = if set_active {
            if self.is_builtin {
                self.selected_scheme_id.clone()
            } else if let Some(c) = self
                .custom_schemes
                .iter()
                .find(|c| c.id == self.selected_scheme_id)
            {
                c.name.clone()
            } else {
                self.selected_scheme_id.clone()
            }
        } else {
            self.active_scheme_name.clone()
        };

        settings_entity(cx).update(cx, |state, cx| {
            state.set_custom_terminal_color_schemes(custom_list, cx);
            if set_active {
                state.set_color_scheme(target_active, cx);
            }
        });

        cx.emit(TerminalColorSchemeDialogEvent::SchemeSaved);
        self.close(cx);
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        cx.emit(TerminalColorSchemeDialogEvent::Close);
    }
}

impl Render for TerminalColorSchemeDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);

        let selected_id = self.selected_scheme_id.clone();
        let is_builtin = self.is_builtin;
        let draft_name_entity = self.draft_name.clone();
        let palette = self.draft_palette;
        let active_picker = self.active_picker.clone();

        modal_content("terminal-color-scheme-dialog", cx)
            .w(px(800.0))
            .max_h(px(660.0))
            .flex()
            .flex_col()
            .track_focus(&self.focus_handle)
            .key_context("TerminalColorSchemeDialog")
            .on_action(cx.listener(|this, _: &Cancel, _window, cx| {
                this.close(cx);
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            })
            // 1. Header
                    .child(modal_header(
                        i18n!(cx, "terminal_color_schemes.title"),
                        None::<&str>,
                        &t,
                        cx,
                        cx.listener(|this, _, _window, cx| this.close(cx)),
                    ))
                    // 2. Main content area (Two-column layout)
                    .child(
                        h_flex()
                            .flex_1()
                            .min_h_0()
                            .overflow_hidden()
                            // Left Column: Scheme navigation list
                            .child(
                                div()
                                    .w(px(230.0))
                                    .h_full()
                                    .border_r_1()
                                    .border_color(p.border_subtle)
                                    .flex()
                                    .flex_col()
                                    .child(
                                        h_flex()
                                            .px(SPACE_XL)
                                            .py(SPACE_SM)
                                            .border_b_1()
                                            .border_color(p.border_subtle)
                                            .justify_between()
                                            .items_center()
                                            .child(
                                                div()
                                                    .text_size(ui_text_md(cx))
                                                    .font_weight(FontWeight::BOLD)
                                                    .text_color(p.text_secondary)
                                                    .child(i18n!(
                                                        cx,
                                                        "terminal_color_schemes.schemes_list"
                                                    )),
                                            )
                                            .child(
                                                Button::new("new-custom-scheme-btn", &t)
                                                    .icon_left(AppIcon::Plus)
                                                    .text()
                                                    .tooltip(i18n!(
                                                        cx,
                                                        "terminal_color_schemes.new_scheme"
                                                    ))
                                                    .on_click(cx.listener(
                                                        |this, _, _window, cx| {
                                                            this.new_custom_scheme(cx);
                                                        },
                                                    )),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("scheme-left-list")
                                            .flex_1()
                                            .overflow_y_scroll()
                                            .px(SPACE_SM)
                                            .py(SPACE_SM)
                                            .flex()
                                            .flex_col()
                                            .gap(px(2.0))
                                            // Built-in section
                                            .child(
                                                div()
                                                    .px(SPACE_MD)
                                                    .py(SPACE_XS)
                                                    .text_size(ui_text_sm(cx))
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(p.text_muted)
                                                    .child(i18n!(
                                                        cx,
                                                        "terminal_color_schemes.builtin_group"
                                                    )),
                                            )
                                            .children(BUILTIN_COLOR_SCHEMES.iter().map(
                                                |&(name, b_palette)| {
                                                    let is_selected = selected_id == name;
                                                    let is_active = self.active_scheme_name == name;
                                                    let name_str = name.to_string();

                                                    h_flex()
                                                        .id(ElementId::Name(
                                                            format!("builtin-scheme-{}", name)
                                                                .into(),
                                                        ))
                                                        .px(SPACE_MD)
                                                        .py(SPACE_SM)
                                                        .rounded(RADIUS_STD)
                                                        .border_1()
                                                        .border_color(if is_selected {
                                                            p.surface_accent.opacity(0.3)
                                                        } else {
                                                            with_alpha(0x00000000, 0.0)
                                                        })
                                                        .cursor_pointer()
                                                        .items_center()
                                                        .justify_between()
                                                        .when(is_selected, |d| {
                                                            d.bg(p.surface_accent.opacity(0.14))
                                                                .text_color(p.text_primary)
                                                        })
                                                        .when(!is_selected, |d| {
                                                            d.text_color(p.text_secondary)
                                                                .hover(|s| s.bg(p.surface_hover))
                                                        })
                                                        .on_mouse_down(
                                                            MouseButton::Left,
                                                            cx.listener(
                                                                move |this, _, _window, cx| {
                                                                    this.select_scheme(
                                                                        &name_str, cx,
                                                                    );
                                                                },
                                                            ),
                                                        )
                                                        .child(
                                                            h_flex()
                                                                .gap(SPACE_SM)
                                                                .items_center()
                                                                // Mini preview dots
                                                                .child(
                                                                    h_flex()
                                                                        .gap(px(2.0))
                                                                        .child(
                                                                            div()
                                                                                .w(px(6.0))
                                                                                .h(px(6.0))
                                                                                .rounded_full()
                                                                                .bg(rgb(
                                                                                    b_palette.red
                                                                                )),
                                                                        )
                                                                        .child(
                                                                            div()
                                                                                .w(px(6.0))
                                                                                .h(px(6.0))
                                                                                .rounded_full()
                                                                                .bg(rgb(
                                                                                    b_palette.green
                                                                                )),
                                                                        )
                                                                        .child(
                                                                            div()
                                                                                .w(px(6.0))
                                                                                .h(px(6.0))
                                                                                .rounded_full()
                                                                                .bg(rgb(
                                                                                    b_palette.blue
                                                                                )),
                                                                        ),
                                                                )
                                                                .child(
                                                                    div()
                                                                        .text_size(ui_text_md(cx))
                                                                        .text_color(
                                                                            if is_selected {
                                                                                p.text_primary
                                                                            } else {
                                                                                p.text_secondary
                                                                            },
                                                                        )
                                                                        .child(name.to_string()),
                                                                ),
                                                        )
                                                        .when(is_active, |d| {
                                                            d.child(
                                                                div()
                                                                    .text_size(ui_text_xs(cx))
                                                                    .text_color(p.status_success)
                                                                    .child("✓"),
                                                            )
                                                        })
                                                },
                                            ))
                                            // Custom schemes section
                                            .child(
                                                div()
                                                    .px(SPACE_MD)
                                                    .pt(SPACE_MD)
                                                    .pb(SPACE_XS)
                                                    .text_size(ui_text_sm(cx))
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(p.text_muted)
                                                    .child(i18n!(
                                                        cx,
                                                        "terminal_color_schemes.custom_group"
                                                    )),
                                            )
                                            .children(self.custom_schemes.iter().map(|custom| {
                                                let is_selected = selected_id == custom.id;
                                                let is_active = self.active_scheme_name
                                                    == custom.name
                                                    || self.active_scheme_name == custom.id;
                                                let id_owned = custom.id.clone();
                                                let name_display = custom.name.clone();
                                                let c_palette = custom.palette;

                                                h_flex()
                                                    .id(ElementId::Name(
                                                        format!("custom-scheme-{}", id_owned)
                                                            .into(),
                                                    ))
                                                    .px(SPACE_MD)
                                                    .py(SPACE_SM)
                                                    .rounded(RADIUS_STD)
                                                    .border_1()
                                                    .border_color(if is_selected {
                                                        p.surface_accent.opacity(0.3)
                                                    } else {
                                                        with_alpha(0x00000000, 0.0)
                                                    })
                                                    .cursor_pointer()
                                                    .items_center()
                                                    .justify_between()
                                                    .when(is_selected, |d| {
                                                        d.bg(p.surface_accent.opacity(0.14))
                                                            .text_color(p.text_primary)
                                                    })
                                                    .when(!is_selected, |d| {
                                                        d.text_color(p.text_secondary)
                                                            .hover(|s| s.bg(p.surface_hover))
                                                    })
                                                    .on_mouse_down(
                                                        MouseButton::Left,
                                                        cx.listener({
                                                            let id = id_owned.clone();
                                                            move |this, _, _window, cx| {
                                                                this.select_scheme(&id, cx);
                                                            }
                                                        }),
                                                    )
                                                    .child(
                                                        h_flex()
                                                            .gap(SPACE_SM)
                                                            .items_center()
                                                            // Mini preview dots
                                                            .child(
                                                                h_flex()
                                                                    .gap(px(2.0))
                                                                    .child(
                                                                        div()
                                                                            .w(px(6.0))
                                                                            .h(px(6.0))
                                                                            .rounded_full()
                                                                            .bg(rgb(c_palette.red)),
                                                                    )
                                                                    .child(
                                                                        div()
                                                                            .w(px(6.0))
                                                                            .h(px(6.0))
                                                                            .rounded_full()
                                                                            .bg(rgb(
                                                                                c_palette.green
                                                                            )),
                                                                    )
                                                                    .child(
                                                                        div()
                                                                            .w(px(6.0))
                                                                            .h(px(6.0))
                                                                            .rounded_full()
                                                                            .bg(
                                                                                rgb(c_palette.blue),
                                                                            ),
                                                                    ),
                                                            )
                                                            .child(
                                                                div()
                                                                    .text_size(ui_text_md(cx))
                                                                    .text_color(if is_selected {
                                                                        p.text_primary
                                                                    } else {
                                                                        p.text_secondary
                                                                    })
                                                                    .child(name_display),
                                                            ),
                                                    )
                                                    .child(
                                                        h_flex()
                                                            .gap(SPACE_XS)
                                                            .items_center()
                                                            .when(is_active, |d| {
                                                                d.child(
                                                                    div()
                                                                        .text_size(ui_text_xs(cx))
                                                                        .text_color(
                                                                            p.status_success,
                                                                        )
                                                                        .child("✓"),
                                                                )
                                                            })
                                                            .child(
                                                                Button::new(
                                                                    format!("del-{}", id_owned),
                                                                    &t,
                                                                )
                                                                .label("×")
                                                                .small()
                                                                .text()
                                                                .tooltip(i18n!(cx, "common.action.delete"))
                                                                .on_click(cx.listener({
                                                                    let id = id_owned.clone();
                                                                    move |this, _, _window, cx| {
                                                                        this.delete_custom_scheme(
                                                                            &id, cx,
                                                                        );
                                                                    }
                                                                })),
                                                            ),
                                                    )
                                            })),
                                    ),
                            )
                            // Right Column: Scheme Details, Live ANSI Terminal Preview & 20-Color Swatch Grid
                            .child(
                                div()
                                    .id("scheme-right-col")
                                    .flex_1()
                                    .h_full()
                                    .overflow_y_scroll()
                                    .p(SPACE_LG)
                                    .flex()
                                    .flex_col()
                                    .gap(SPACE_MD)
                                    // 1. Name & Duplicate Action Header
                                    .child(
                                        h_flex()
                                            .justify_between()
                                            .items_center()
                                            .gap(SPACE_MD)
                                            .child(
                                                h_flex()
                                                    .flex_1()
                                                    .gap(SPACE_SM)
                                                    .items_center()
                                                    .child(
                                                        div()
                                                            .text_size(ui_text_md(cx))
                                                            .font_weight(FontWeight::BOLD)
                                                            .text_color(p.text_secondary)
                                                            .child(i18n!(
                                                                cx,
                                                                "terminal_color_schemes.scheme_name"
                                                            )),
                                                    )
                                                    .child(div().flex_1().child(SimpleInput::new(
                                                        &draft_name_entity,
                                                    ))),
                                            )
                                            .child(
                                                Button::new("duplicate-scheme-btn", &t)
                                                    .icon_left(AppIcon::Copy)
                                                    .default()
                                                    .tooltip(i18n!(
                                                        cx,
                                                        "terminal_color_schemes.duplicate_tooltip"
                                                    ))
                                                    .on_click(cx.listener(
                                                        |this, _, _window, cx| {
                                                            this.duplicate_current_scheme(cx);
                                                        },
                                                    )),
                                            ),
                                    )
                                    // 2. Color Swatches Form Grid (Above)
                                    .child(
                                        self.render_swatches_section(&palette, is_builtin, &t, cx),
                                    )
                                    // 3. Live ANSI Terminal Preview (Below)
                                    .child(self.render_preview_card(&palette, &t, cx)),
                            ),
                    )
                    // 3. Footer actions
                    .child(
                        h_flex()
                            .h(px(48.0))
                            .flex_shrink_0()
                            .px(SPACE_LG)
                            .border_t_1()
                            .border_color(p.border_subtle)
                            .justify_end()
                            .items_center()
                            .gap(SPACE_MD)
                            .child(
                                Button::new("cancel-scheme-btn", &t)
                                    .label(i18n!(cx, "common.action.cancel"))
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.close(cx);
                                    })),
                            )
                            .child(
                                Button::new("save-scheme-btn", &t)
                                    .label(i18n!(cx, "common.action.save"))
                                    .disabled(is_builtin)
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.save_and_apply(false, cx);
                                    })),
                            )
                            .child(
                                Button::new("apply-scheme-btn", &t)
                                    .primary()
                                    .label(i18n!(cx, "terminal_color_schemes.set_as_active_btn"))
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.save_and_apply(true, cx);
                                    })),
                            ),
                    )
            .when_some(active_picker, |d, picker| d.child(picker))
    }
}

impl TerminalColorSchemeDialog {
    /// Render the live terminal ANSI simulation card
    fn render_preview_card(
        &self,
        p: &TerminalPalette,
        t: &velowork_core::theme::ThemeColors,
        cx: &App,
    ) -> impl IntoElement {
        let cursor_c = p.cursor.unwrap_or(p.foreground);
        let selection_c = p.selection.unwrap_or(0x264F78);

        div()
            .flex()
            .flex_col()
            .gap(SPACE_XS)
            .child(
                div()
                    .text_size(ui_text_md(cx))
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(t.text_secondary))
                    .child(i18n!(cx, "terminal_color_schemes.preview_title")),
            )
            .child(
                div()
                    .rounded(RADIUS_LG)
                    .bg(rgb(p.background))
                    .p(SPACE_MD)
                    .border_1()
                    .border_color(rgb(0x333333))
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    // Line 1: Prompt & Command
                    .child(
                        h_flex()
                            .text_size(ui_text_sm(cx))
                            .child(div().text_color(rgb(p.green)).child("user@velowork"))
                            .child(div().text_color(rgb(p.foreground)).child(":"))
                            .child(div().text_color(rgb(p.blue)).child("~"))
                            .child(
                                div()
                                    .text_color(rgb(p.foreground))
                                    .child("$ ls -la --color=auto"),
                            ),
                    )
                    // Line 2: Directory entry (Blue)
                    .child(
                        h_flex()
                            .text_size(ui_text_sm(cx))
                            .child(
                                div()
                                    .text_color(rgb(p.bright_black))
                                    .child("drwxr-xr-x 4 user user 4096 Aug 27 14:00 "),
                            )
                            .child(
                                div()
                                    .text_color(rgb(p.bright_blue))
                                    .font_weight(FontWeight::BOLD)
                                    .child("src/"),
                            ),
                    )
                    // Line 3: Executable entry (Green)
                    .child(
                        h_flex()
                            .text_size(ui_text_sm(cx))
                            .child(
                                div()
                                    .text_color(rgb(p.bright_black))
                                    .child("-rwxr-xr-x 1 user user 1824 Aug 27 14:02 "),
                            )
                            .child(
                                div()
                                    .text_color(rgb(p.bright_green))
                                    .font_weight(FontWeight::BOLD)
                                    .child("app*"),
                            ),
                    )
                    // Line 4: Symlink entry (Cyan) & Warn/Error (Yellow/Red)
                    .child(
                        h_flex()
                            .text_size(ui_text_sm(cx))
                            .child(
                                div()
                                    .text_color(rgb(p.bright_black))
                                    .child("lrwxrwxrwx 1 user user   12 Aug 27 14:05 "),
                            )
                            .child(div().text_color(rgb(p.cyan)).child("config -> "))
                            .child(div().text_color(rgb(p.magenta)).child("/etc/app")),
                    )
                    // Line 5: Simulated selection highlight & cursor
                    .child(
                        h_flex()
                            .text_size(ui_text_sm(cx))
                            .child(div().text_color(rgb(p.green)).child("user@velowork"))
                            .child(div().text_color(rgb(p.foreground)).child(":"))
                            .child(div().text_color(rgb(p.blue)).child("~"))
                            .child(div().text_color(rgb(p.foreground)).child("$ echo \""))
                            .child(
                                div()
                                    .bg(rgb(selection_c))
                                    .text_color(rgb(p.foreground))
                                    .child("Velowork Terminal"),
                            )
                            .child(div().text_color(rgb(p.foreground)).child("\""))
                            // Simulated cursor block
                            .child(div().w(px(8.0)).h(px(14.0)).bg(rgb(cursor_c)).opacity(0.85)),
                    )
                    // 16 ANSI Color Swatches strip
                    .child(
                        h_flex().mt(SPACE_SM).gap(px(4.0)).children(
                            [
                                p.black,
                                p.red,
                                p.green,
                                p.yellow,
                                p.blue,
                                p.magenta,
                                p.cyan,
                                p.white,
                                p.bright_black,
                                p.bright_red,
                                p.bright_green,
                                p.bright_yellow,
                                p.bright_blue,
                                p.bright_magenta,
                                p.bright_cyan,
                                p.bright_white,
                            ]
                            .iter()
                            .map(|&c| div().flex_1().h(px(12.0)).rounded(RADIUS_STD).bg(rgb(c))),
                        ),
                    ),
            )
    }

    /// Render all 20 color swatches divided into sections
    fn render_swatches_section(
        &self,
        p: &TerminalPalette,
        is_builtin: bool,
        t: &velowork_core::theme::ThemeColors,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap(SPACE_MD)
            // 1. Core functional colors
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(SPACE_XS)
                    .child(
                        div()
                            .text_size(ui_text_md(cx))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(t.text_secondary))
                            .child(i18n!(cx, "terminal_color_schemes.core_colors")),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(SPACE_SM)
                            .child(self.render_swatch_card(
                                "foreground",
                                i18n!(cx, "terminal_color_schemes.foreground"),
                                p.foreground,
                                is_builtin,
                                t,
                                cx,
                            ))
                            .child(self.render_swatch_card(
                                "background",
                                i18n!(cx, "terminal_color_schemes.background"),
                                p.background,
                                is_builtin,
                                t,
                                cx,
                            ))
                            .child(self.render_swatch_card(
                                "cursor",
                                i18n!(cx, "terminal_color_schemes.cursor"),
                                p.cursor.unwrap_or(p.foreground),
                                is_builtin,
                                t,
                                cx,
                            ))
                            .child(self.render_swatch_card(
                                "selection",
                                i18n!(cx, "terminal_color_schemes.selection"),
                                p.selection.unwrap_or(0x264F78),
                                is_builtin,
                                t,
                                cx,
                            )),
                    ),
            )
            // 2. Standard 8 ANSI colors
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(SPACE_XS)
                    .child(
                        div()
                            .text_size(ui_text_md(cx))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(t.text_secondary))
                            .child(i18n!(cx, "terminal_color_schemes.standard_ansi")),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(SPACE_SM)
                            .child(
                                self.render_swatch_card(
                                    "black", "Black", p.black, is_builtin, t, cx,
                                ),
                            )
                            .child(self.render_swatch_card("red", "Red", p.red, is_builtin, t, cx))
                            .child(
                                self.render_swatch_card(
                                    "green", "Green", p.green, is_builtin, t, cx,
                                ),
                            )
                            .child(self.render_swatch_card(
                                "yellow", "Yellow", p.yellow, is_builtin, t, cx,
                            ))
                            .child(
                                self.render_swatch_card("blue", "Blue", p.blue, is_builtin, t, cx),
                            )
                            .child(self.render_swatch_card(
                                "magenta", "Magenta", p.magenta, is_builtin, t, cx,
                            ))
                            .child(
                                self.render_swatch_card("cyan", "Cyan", p.cyan, is_builtin, t, cx),
                            )
                            .child(
                                self.render_swatch_card(
                                    "white", "White", p.white, is_builtin, t, cx,
                                ),
                            ),
                    ),
            )
            // 3. Bright 8 ANSI colors
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(SPACE_XS)
                    .child(
                        div()
                            .text_size(ui_text_md(cx))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(t.text_secondary))
                            .child(i18n!(cx, "terminal_color_schemes.bright_ansi")),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(SPACE_SM)
                            .child(self.render_swatch_card(
                                "bright_black",
                                "Bright Black",
                                p.bright_black,
                                is_builtin,
                                t,
                                cx,
                            ))
                            .child(self.render_swatch_card(
                                "bright_red",
                                "Bright Red",
                                p.bright_red,
                                is_builtin,
                                t,
                                cx,
                            ))
                            .child(self.render_swatch_card(
                                "bright_green",
                                "Bright Green",
                                p.bright_green,
                                is_builtin,
                                t,
                                cx,
                            ))
                            .child(self.render_swatch_card(
                                "bright_yellow",
                                "Bright Yellow",
                                p.bright_yellow,
                                is_builtin,
                                t,
                                cx,
                            ))
                            .child(self.render_swatch_card(
                                "bright_blue",
                                "Bright Blue",
                                p.bright_blue,
                                is_builtin,
                                t,
                                cx,
                            ))
                            .child(self.render_swatch_card(
                                "bright_magenta",
                                "Bright Magenta",
                                p.bright_magenta,
                                is_builtin,
                                t,
                                cx,
                            ))
                            .child(self.render_swatch_card(
                                "bright_cyan",
                                "Bright Cyan",
                                p.bright_cyan,
                                is_builtin,
                                t,
                                cx,
                            ))
                            .child(self.render_swatch_card(
                                "bright_white",
                                "Bright White",
                                p.bright_white,
                                is_builtin,
                                t,
                                cx,
                            )),
                    ),
            )
    }

    /// Render a single color swatch card with name and interactive click-to-pick
    fn render_swatch_card(
        &self,
        key: &'static str,
        label: impl Into<String>,
        color: u32,
        is_builtin: bool,
        _t: &velowork_core::theme::ThemeColors,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let label_str = label.into();
        let key_str = key.to_string();
        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);

        let mut card = h_flex()
            .id(ElementId::Name(format!("swatch-card-{}", key).into()))
            .w(px(124.0))
            .p(SPACE_XS)
            .rounded(RADIUS_STD)
            .border_1()
            .border_color(p.border_subtle)
            .bg(p.surface_card)
            .items_center()
            .gap(SPACE_SM);

        if is_builtin {
            let tip = i18n!(cx, "terminal_color_schemes.builtin_readonly_tip");
            card = card
                .cursor_default()
                .tooltip(move |_, cx| cx.new(|_| Tooltip::new(tip.clone())).into());
        } else {
            card = card
                .cursor_pointer()
                .hover(|s| s.bg(p.surface_hover))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener({
                        let key_str = key_str.clone();
                        move |this, event: &MouseDownEvent, _window, cx| {
                            cx.stop_propagation();
                            this.open_color_picker(&key_str, color, event.position, cx);
                        }
                    }),
                );
        }

        card
            // Swatch block
            .child(
                div()
                    .w(px(20.0))
                    .h(px(20.0))
                    .rounded(RADIUS_STD)
                    .bg(rgb(color))
                    .border_1()
                    .border_color(p.border_subtle),
            )
            // Name label only
            .child(
                div()
                    .flex_1()
                    .text_size(ui_text_sm(cx))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(p.text_primary)
                    .child(label_str),
            )
    }
}
