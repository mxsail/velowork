use crate::settings::settings_entity;
use crate::theme::theme;
use crate::workspace::settings::{
    ColorSchema, ColorTheme, CustomTitlebarPosition, CustomTitlebarPreset, TabWidthMode, TitlebarStyle,
};
use gpui::prelude::*;
use gpui::*;
use velowork_i18n::i18n;

use velowork_ui::select::Select;

use super::SettingsPanel;
use super::components::*;

impl SettingsPanel {
    pub(super) fn render_appearance(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let s = settings_entity(cx).read(cx).settings.clone();

        // Section: Theme
        let theme_section = {
            let _color_theme_label = i18n!(cx, "settings.color_schema.label");
            let bg_opacity_label = i18n!(cx, "settings.bg_opacity");

            section_container(&t)
                .child(self.render_color_theme_row(s.color_schema, cx))
                .child(self.render_color_palette_row(
                    "settings.dark_color_theme",
                    s.dark_color_theme,
                    true,
                    cx,
                ))
                .child(self.render_color_palette_row(
                    "settings.light_color_theme",
                    s.light_color_theme,
                    false,
                    cx,
                ))
                .child(self.render_slider(
                    "bg-opacity",
                    &bg_opacity_label,
                    s.bg_opacity * 100.0,
                    10.0,
                    100.0,
                    1.0,
                    false,
                    |v| format!("{}%", v.round() as i32),
                    |state, val, cx| state.set_bg_opacity(val / 100.0, cx),
                    cx,
                ))
        };

        // Section: UI Elements
        let ui_section = {
            let show_focus_border_label = i18n!(cx, "settings.show_focus_border");
            let enable_animations_label = i18n!(cx, "settings.appearance.enable_animations");
            let color_tinted_bg_label = i18n!(cx, "settings.color_tinted_bg");
            let _tab_width_mode_label = i18n!(cx, "settings.tab_width_mode.label");

            section_container(&t)
                .child(self.render_toggle(
                    "focus-border",
                    &show_focus_border_label,
                    s.show_focused_border,
                    true,
                    |state, val, cx| state.set_show_focused_border(val, cx),
                    cx,
                ))
                .child(self.render_toggle(
                    "enable-animations",
                    &enable_animations_label,
                    s.enable_animations,
                    true,
                    |state, val, cx| state.set_enable_animations(val, cx),
                    cx,
                ))
                .child(self.render_toggle(
                    "color-tinted-bg",
                    &color_tinted_bg_label,
                    s.color_tinted_background,
                    true,
                    |state, val, cx| state.set_color_tinted_background(val, cx),
                    cx,
                ))
                .child(self.render_titlebar_style_row(s.titlebar_style, cx))
                .when(s.titlebar_style == TitlebarStyle::Custom, |d| {
                    d.child(self.render_titlebar_preset_row(s.titlebar_preset, cx))
                        .child(self.render_titlebar_position_row(s.titlebar_position, cx))
                        .child(self.render_number_stepper(
                            "titlebar-height",
                            &i18n!(cx, "settings.titlebar_height"),
                            s.titlebar_height,
                            "{}px",
                            20.0,
                            60.0,
                            1.0,
                            60.0,
                            false,
                            |state, val, cx| state.set_titlebar_height(val, cx),
                            window,
                            cx,
                        ))
                        .child(self.render_number_stepper(
                            "button-gap",
                            &i18n!(cx, "settings.window_control_button_gap"),
                            s.window_control_button_gap,
                            "{}px",
                            0.0,
                            32.0,
                            1.0,
                            60.0,
                            false,
                            |state, val, cx| state.set_window_control_button_gap(val, cx),
                            window,
                            cx,
                        ))
                        .child(self.render_number_stepper(
                            "control-margin",
                            &i18n!(cx, "settings.window_control_margin"),
                            s.window_control_margin,
                            "{}px",
                            0.0,
                            48.0,
                            1.0,
                            60.0,
                            false,
                            |state, val, cx| state.set_window_control_margin(val, cx),
                            window,
                            cx,
                        ))
                        .child(self.render_window_corner_radius_row(s.window_corner_radius, window, cx))
                        .child(self.render_window_control_icon_size_row(s.window_control_icon_size, window, cx))
                })
                .child(self.render_ui_density_row(s.ui_density, cx))
                .child(self.render_tab_width_mode_row(s.tab_width_mode, cx))
                .child(self.render_toggle_with_desc(
                    "enable-tab-preview",
                    &i18n!(cx, "settings.appearance.enable_tab_preview"),
                    &i18n!(cx, "settings.appearance.enable_tab_preview_desc"),
                    s.enable_tab_preview,
                    true,
                    |state, val, cx| state.set_enable_tab_preview(val, cx),
                    cx,
                ))
        };

        let theme_label = i18n!(cx, "settings.color_schema.label");
        let ui_label = i18n!(cx, "settings.nav.appearance");

        div()
            .child(section_header(&theme_label, &t, cx))
            .child(theme_section)
            .child(section_header(&ui_label, &t, cx))
            .child(ui_section)
    }

    /// Render the appearance-mode (color schema) selector as an inline
    /// segmented control. The segments come from `ColorSchema::all_variants()`.
    fn render_color_theme_row(
        &mut self,
        current: ColorSchema,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx);
        let label = i18n!(cx, "settings.color_schema.label");
        let focus_handle = self.get_or_create_radio_focus_handle("color-theme", cx);

        settings_row("color-theme".to_string(), &label, &t, cx, true).child(
            RadioGroup::new("color-theme-radio")
                .focus(&focus_handle)
                .mode(RadioMode::Button)
                .selected(Some(current))
                .options(
                    ColorSchema::all_variants()
                        .iter()
                        .map(|&theme| RadioOption::new(theme, i18n!(cx, theme.translation_key())))
                        .collect(),
                )
                .on_change(move |&theme, _, cx| {
                    settings_entity(cx).update(cx, |state, cx| {
                        state.set_color_theme(theme, cx);
                    });
                }),
        )
    }

    /// Render the UI density selector (Compact / Default / Comfortable)
    fn render_ui_density_row(
        &mut self,
        current: velowork_ui::UiDensity,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx);
        let label = i18n!(cx, "settings.ui_density.label");
        let focus_handle = self.get_or_create_radio_focus_handle("ui-density", cx);

        let options = vec![
            RadioOption::new(velowork_ui::UiDensity::Compact, i18n!(cx, "settings.ui_density.compact")),
            RadioOption::new(velowork_ui::UiDensity::Default, i18n!(cx, "common.default")),
            RadioOption::new(velowork_ui::UiDensity::Comfortable, i18n!(cx, "settings.ui_density.comfortable")),
        ];

        settings_row("ui-density".to_string(), &label, &t, cx, true).child(
            RadioGroup::new("ui-density-radio")
                .focus(&focus_handle)
                .mode(RadioMode::Button)
                .selected(Some(current))
                .options(options)
                .on_change(move |&density, _, cx| {
                    settings_entity(cx).update(cx, |state, cx| {
                        state.set_ui_density(density, cx);
                    });
                }),
        )
    }

    /// Render the palette picker for one appearance side (dark or light) as a
    /// dropdown. Editing it writes `dark_color_theme` / `light_color_theme`
    /// directly. The option list comes from `ColorTheme::all_variants()`, so
    /// new palettes (built-in or custom) appear automatically.
    fn render_color_palette_row(
        &self,
        label_key: &'static str,
        _current: ColorTheme,
        is_dark: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx);
        let label = i18n!(cx, label_key);
        let side = if is_dark { "dark" } else { "light" };

        let select_el = if is_dark {
            Select::new(&self.dark_palette_select)
        } else {
            Select::new(&self.light_palette_select)
        };

        settings_row(format!("palette-{}-theme", side), &label, &t, cx, true)
            .child(div().w(px(180.0)).child(select_el))
    }

    fn render_tab_width_mode_row(
        &mut self,
        current: TabWidthMode,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx);
        let label = i18n!(cx, "settings.tab_width_mode.label");
        let focus_handle = self.get_or_create_radio_focus_handle("tab-width-mode", cx);

        settings_row("tab-width-mode".to_string(), &label, &t, cx, true).child(
            RadioGroup::new("tab-width-mode-radio")
                .focus(&focus_handle)
                .mode(RadioMode::Button)
                .selected(Some(current))
                .options(
                    TabWidthMode::all_variants()
                        .iter()
                        .map(|&mode| RadioOption::new(mode, i18n!(cx, mode.translation_key())))
                        .collect(),
                )
                .on_change(move |&mode, _, cx| {
                    settings_entity(cx).update(cx, |state, cx| {
                        state.set_tab_width_mode(mode, cx);
                    });
                }),
        )
    }

    /// Render the Titlebar style selector (Custom / Native)
    fn render_titlebar_style_row(
        &mut self,
        current: TitlebarStyle,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx);
        let label = i18n!(cx, "settings.titlebar_style.label");
        let desc = i18n!(cx, "common.requires_restart");
        let focus_handle = self.get_or_create_radio_focus_handle("titlebar-style", cx);

        settings_row_with_desc("titlebar-style".to_string(), &label, &desc, &t, cx, true).child(
            RadioGroup::new("titlebar-style-radio")
                .focus(&focus_handle)
                .mode(RadioMode::Button)
                .selected(Some(current))
                .options(
                    TitlebarStyle::all_variants()
                        .iter()
                        .map(|&style| RadioOption::new(style, i18n!(cx, style.translation_key())))
                        .collect(),
                )
                .on_change(move |&style, _, cx| {
                    if style != current {
                        settings_entity(cx).update(cx, |state, cx| {
                            state.set_titlebar_style(style, cx);
                        });
                        let toast = velowork_workspace::toast::Toast::warning(
                            i18n!(cx, "settings.titlebar_style_changed_notice"),
                        )
                        .with_actions(vec![velowork_workspace::toast::ToastAction::new(
                            "restart_app",
                            i18n!(cx, "update.restart"),
                            velowork_workspace::toast::ToastActionStyle::Primary,
                        )]);
                        velowork_workspace::toast::ToastManager::post(toast, cx);
                    }
                }),
        )
    }
    /// Render the Window corner radius control for custom titlebar mode
    fn render_window_corner_radius_row(
        &mut self,
        current: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        self.render_integer_stepper_with_desc(
            "window-corner-radius",
            i18n!(cx, "settings.window_corner_radius"),
            i18n!(cx, "settings.window_corner_radius_desc"),
            current as u32,
            0,
            32,
            1,
            60.0,
            true,
            |state, val, cx| {
                state.set_window_corner_radius(val as f32, cx);
            },
            window,
            cx,
        )
    }

    /// Render the Window control icon size adjustment row for custom titlebar mode
    fn render_window_control_icon_size_row(
        &mut self,
        current: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        self.render_integer_stepper_with_desc(
            "window-control-icon-size",
            i18n!(cx, "settings.window_control_icon_size"),
            i18n!(cx, "settings.window_control_icon_size_desc"),
            current as u32,
            8,
            32,
            1,
            60.0,
            true,
            |state, val, cx| {
                state.set_window_control_icon_size(val as f32, cx);
            },
            window,
            cx,
        )
    }

    fn render_titlebar_preset_row(
        &self,
        _current: CustomTitlebarPreset,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx);
        let label = i18n!(cx, "settings.titlebar_preset.label");

        settings_row("titlebar-preset".to_string(), &label, &t, cx, true)
            .child(div().w(px(180.0)).child(Select::new(&self.titlebar_preset_select)))
    }

    fn render_titlebar_position_row(
        &mut self,
        current: CustomTitlebarPosition,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx);
        let label = i18n!(cx, "settings.titlebar_position.label");
        let focus_handle = self.get_or_create_radio_focus_handle("titlebar-position", cx);

        settings_row("titlebar-position".to_string(), &label, &t, cx, true).child(
            RadioGroup::new("titlebar-position-radio")
                .focus(&focus_handle)
                .mode(RadioMode::Button)
                .selected(Some(current))
                .options(
                    CustomTitlebarPosition::all_variants()
                        .iter()
                        .map(|&pos| RadioOption::new(pos, i18n!(cx, pos.translation_key())))
                        .collect(),
                )
                .on_change(move |&pos, _, cx| {
                    settings_entity(cx).update(cx, |state, cx| {
                        state.set_titlebar_position(pos, cx);
                    });
                }),
        )
    }
}
