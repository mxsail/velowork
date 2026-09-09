use crate::keybindings::Cancel;
use crate::settings::settings_entity;
use crate::theme::{
    load_custom_themes, theme, theme_entity,
    ThemeColors, ThemeInfo, ColorTheme, DARK_THEME, LIGHT_THEME,
    HIGH_CONTRAST_THEME, PASTEL_DARK_THEME,
};
use crate::views::components::{
    badge, keyboard_hints_footer, modal_content, modal_header,
    QuickPickerConfig, QuickPickerState,
};
use crate::ui::tokens::{ui_text, ui_text_md, ui_text_xl};
use velowork_ui::tokens::{SPACE_XS, SPACE_MD, SPACE_LG, RADIUS_STD};
use gpui::*;
use velowork_ui::h_flex;
use gpui::prelude::*;
use velowork_ui::selectable_list::selectable_list_item;
use velowork_i18n::i18n;

/// Theme selection entry with preview and info
#[derive(Clone)]
struct ThemeEntry {
    info: ThemeInfo,
    colors: ThemeColors,
}

/// Theme selector overlay for choosing and previewing themes
pub struct ThemeSelector {
    focus_handle: FocusHandle,
    state: QuickPickerState<ThemeEntry>,
}

impl ThemeSelector {
    pub fn new(cx: &mut Context<Self>) -> Self {
        // Build theme list: built-in + custom
        let mut themes = vec![
            // Add built-in themes
        ];

        themes.push(ThemeEntry {
            info: ThemeInfo {
                id: "dark".to_string(),
                name: "Dark".to_string(),
                description: "Modern high-end dark theme".to_string(),
                is_dark: true,
            },
            colors: DARK_THEME,
        });

        themes.push(ThemeEntry {
            info: ThemeInfo {
                id: "light".to_string(),
                name: "Light".to_string(),
                description: "Modern high-end light theme".to_string(),
                is_dark: false,
            },
            colors: LIGHT_THEME,
        });

        themes.push(ThemeEntry {
            info: ThemeInfo {
                id: "pastel-dark".to_string(),
                name: "Pastel Dark".to_string(),
                description: "Soft pastel colors on dark background".to_string(),
                is_dark: true,
            },
            colors: PASTEL_DARK_THEME,
        });

        themes.push(ThemeEntry {
            info: ThemeInfo {
                id: "high-contrast".to_string(),
                name: "High Contrast".to_string(),
                description: "High contrast for better visibility".to_string(),
                is_dark: true,
            },
            colors: HIGH_CONTRAST_THEME,
        });

        // Add custom themes
        for (info, colors) in load_custom_themes() {
            themes.push(ThemeEntry { info, colors });
        }

        // Find current theme index based on the *active* appearance's palette.
        let current = theme_entity(cx).read(cx).active_color_theme();
        let selected_index = match current {
            ColorTheme::Dark => 0,
            ColorTheme::Light => 1,
            ColorTheme::PastelDark => 2,
            ColorTheme::HighContrast => 3,
            ColorTheme::Custom => {
                // Try to find matching custom theme
                themes.iter().position(|t| t.info.id.starts_with("custom:"))
                    .unwrap_or(0)
            }
        };

        let config = QuickPickerConfig::new(i18n!(cx, "theme.selector_title"))
            .subtitle(i18n!(cx, "theme.selector_subtitle"))
            .size(480.0, 550.0)
            .centered()
            .keyboard_hints(vec![
                ("Enter".to_string(), i18n!(cx, "common.select")),
                ("Esc".to_string(), i18n!(cx, "common.close")),
            ])
            .key_context("ThemeSelector");

        let state = QuickPickerState::with_selected(themes, config, selected_index, cx);
        let focus_handle = state.focus_handle.clone();

        Self {
            focus_handle,
            state,
        }
    }

    fn close(&self, cx: &mut Context<Self>) {
        // Clear any preview before closing
        theme_entity(cx).update(cx, |theme, _cx| {
            theme.clear_preview();
        });
        cx.emit(ThemeSelectorEvent::Close);
    }

    fn select_theme(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.state.items.len() {
            return;
        }

        let theme_entry = &self.state.items[index];
        let theme_ent = theme_entity(cx);

        // Determine the palette from the theme ID. There is no `Auto` option:
        // the active appearance (and therefore which of the dark/light palettes
        // is shown) is decided by `color_schema` + system appearance.
        let (ct, custom_colors) = match theme_entry.info.id.as_str() {
            "dark" => (ColorTheme::Dark, None),
            "light" => (ColorTheme::Light, None),
            "pastel-dark" => (ColorTheme::PastelDark, None),
            "high-contrast" => (ColorTheme::HighContrast, None),
            id if id.starts_with("custom:") => (ColorTheme::Custom, Some(theme_entry.colors)),
            _ => (ColorTheme::Dark, None),
        };

        // Apply custom colors immediately for the preview of the Custom entry.
        theme_ent.update(cx, |theme, _cx| {
            theme.clear_preview();
            if let Some(colors) = custom_colors {
                theme.set_custom_colors(colors);
            }
        });

        // Persist to the *active* appearance's palette. The settings observer in
        // `init_theme` syncs `AppTheme` from settings, so the chosen palette is
        // applied — and in `System` mode the correct one is picked by the OS.
        let active_is_dark = theme_ent.read(cx).effective_is_dark();
        let custom_id = if ct == ColorTheme::Custom {
            // Extract file stem from "custom:stem" ID
            theme_entry.info.id.strip_prefix("custom:").map(|s| s.to_string())
        } else {
            None
        };
        settings_entity(cx).update(cx, |s, cx| {
            if active_is_dark {
                s.set_dark_color_theme(ct, cx);
            } else {
                s.set_light_color_theme(ct, cx);
            }
            if custom_id.is_some() {
                s.set_custom_theme_id(custom_id, cx);
            }
        });

        self.state.selected_index = index;
        cx.notify();

        // Close the dialog
        cx.emit(ThemeSelectorEvent::Close);
    }

    fn preview_theme(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.state.items.len() {
            return;
        }

        let theme_entry = &self.state.items[index];
        let theme_ent = theme_entity(cx);

        // Determine the palette for preview
        let ct = match theme_entry.info.id.as_str() {
            "dark" => ColorTheme::Dark,
            "light" => ColorTheme::Light,
            "pastel-dark" => ColorTheme::PastelDark,
            "high-contrast" => ColorTheme::HighContrast,
            id if id.starts_with("custom:") => {
                // For custom themes, set the preview colors directly
                theme_ent.update(cx, |theme, cx| {
                    theme.set_preview_colors(theme_entry.colors);
                    cx.notify();
                });
                return;
            }
            _ => ColorTheme::Dark,
        };

        // Set preview for built-in themes
        theme_ent.update(cx, |theme, cx| {
            theme.set_preview(ct);
            cx.notify();
        });
    }

    fn render_theme_preview(&self, colors: &ThemeColors, cx: &App) -> impl IntoElement {
        // Mini terminal preview with the theme colors
        div()
            .w(px(80.0))
            .h(px(50.0))
            .rounded(RADIUS_STD)
            .bg(rgb(colors.bg_primary))
            .border_1()
            .border_color(rgb(colors.border))
            .p(SPACE_XS)
            .flex()
            .flex_col()
            .gap(px(2.0))
            .overflow_hidden()
            .child(
                // Fake title bar
                div()
                    .h(px(8.0))
                    .rounded(px(2.0))
                    .bg(rgb(colors.bg_header))
                    .flex()
                    .items_center()
                    .gap(px(2.0))
                    .px(px(2.0))
                    .child(div().w(px(4.0)).h(px(4.0)).rounded_full().bg(rgb(colors.error)))
                    .child(div().w(px(4.0)).h(px(4.0)).rounded_full().bg(rgb(colors.warning)))
                    .child(div().w(px(4.0)).h(px(4.0)).rounded_full().bg(rgb(colors.success))),
            )
            .child(
                // Fake terminal content
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap(px(1.0))
                    .child(
                        h_flex()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .text_size(ui_text(6.0, cx))
                                    .text_color(rgb(colors.success))
                                    .child("$"),
                            )
                            .child(
                                div()
                                    .text_size(ui_text(6.0, cx))
                                    .text_color(rgb(colors.text_primary))
                                    .child("ls"),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(SPACE_XS)
                            .child(
                                div()
                                    .text_size(ui_text(5.0, cx))
                                    .text_color(rgb(colors.accent))
                                    .child("src"),
                            )
                            .child(
                                div()
                                    .text_size(ui_text(5.0, cx))
                                    .text_color(rgb(colors.text_primary))
                                    .child("Cargo.toml"),
                            ),
                    ),
            )
    }

    fn render_theme_row(&self, index: usize, entry: &ThemeEntry, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t = theme(cx);
        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);
        let is_selected = index == self.state.selected_index;
        let colors = entry.colors;
        let (name, description) = match entry.info.id.as_str() {
            "dark" => (i18n!(cx, "settings.color_theme.dark"), i18n!(cx, "theme.desc_dark")),
            "light" => (i18n!(cx, "settings.color_theme.light"), i18n!(cx, "theme.desc_light")),
            "pastel-dark" => (i18n!(cx, "settings.color_theme.pastel_dark"), i18n!(cx, "theme.desc_pastel_dark")),
            "high-contrast" => (i18n!(cx, "settings.color_theme.high_contrast"), i18n!(cx, "theme.desc_high_contrast")),
            _ => (entry.info.name.clone(), entry.info.description.clone()),
        };
        let is_custom = entry.info.id.starts_with("custom:");

        selectable_list_item(
                ElementId::Name(format!("theme-{}", index).into()),
                is_selected,
                &t,
            )
            .gap(SPACE_LG)
            .py(px(10.0))
            .border_b_1()
            .border_color(p.border_subtle)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _window, cx| {
                    // Preview on click before selection
                    this.preview_theme(index, cx);
                    this.select_theme(index, cx);
                }),
            )
            .child(self.render_theme_preview(&colors, cx))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(
                        h_flex()
                            .gap(SPACE_MD)
                            .child(
                                div()
                                    .text_size(ui_text_xl(cx))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(t.text_primary))
                                    .child(name),
                            )
                            .when(is_custom, |d| {
                                d.child(badge(i18n!(cx, "common.custom"), &t, cx))
                            })
                            .when(is_selected, |d| {
                                d.child(
                                    div()
                                        .text_size(ui_text_md(cx))
                                        .text_color(rgb(t.border_active))
                                        .child("✓"),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_size(ui_text_md(cx))
                            .text_color(rgb(t.text_muted))
                            .child(description),
                    ),
            )
    }
}

pub enum ThemeSelectorEvent {
    Close,
}

impl EventEmitter<ThemeSelectorEvent> for ThemeSelector {}

impl Render for ThemeSelector {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let focus_handle = self.focus_handle.clone();
        let config_width = self.state.config.width;
        let config_max_height = self.state.config.max_height;
        let config_title = self.state.config.title.clone();
        let config_subtitle = self.state.config.subtitle.clone();

        modal_content("theme-selector-modal", cx)
            .w(px(config_width))
            .max_h(px(config_max_height))
            .track_focus(&focus_handle)
            .key_context("ThemeSelector")
            .on_action(cx.listener(|this, _: &Cancel, _window, cx| {
                this.close(cx);
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                match event.keystroke.key.as_str() {
                    "escape" => this.close(cx),
                    "up" | "arrowup" => {
                        if this.state.select_prev() {
                            this.preview_theme(this.state.selected_index, cx);
                            cx.notify();
                        }
                    }
                    "down" | "arrowdown" => {
                        if this.state.select_next() {
                            this.preview_theme(this.state.selected_index, cx);
                            cx.notify();
                        }
                    }
                    "enter" => {
                        let index = this.state.selected_index;
                        this.select_theme(index, cx);
                    }
                    _ => {}
                }
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            })
            .child(modal_header(
                config_title,
                config_subtitle,
                &t,
                cx,
                cx.listener(|this, _, _window, cx| this.close(cx)),
            ))
            .child(
                // Theme list
                div()
                    .id("theme-list")
                    .flex_1()
                    .overflow_y_scroll()
                    .children(
                        self.state.filtered.iter().enumerate().map(|(i, filter_result)| {
                            let entry = &self.state.items[filter_result.index];
                            self.render_theme_row(i, entry, cx)
                        }),
                    ),
            )
            .child({
                let hint_nav = i18n!(cx, "theme.hint_navigate");
                let hint_sel = i18n!(cx, "theme.hint_select");
                let hint_cls = i18n!(cx, "common.close");
                let hints = [
                    ("↑↓", hint_nav.as_str()),
                    ("Enter", hint_sel.as_str()),
                    ("Esc", hint_cls.as_str()),
                ];
                keyboard_hints_footer(&hints, &t, cx)
            })
    }
}

impl_focusable!(ThemeSelector);
