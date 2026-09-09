use crate::settings::settings_entity;
use crate::theme::theme;
use gpui::*;
use velowork_i18n::i18n;
use velowork_ui::tokens::SPACE_XL;

use super::SettingsPanel;
use super::components::*;

impl SettingsPanel {
    pub(super) fn render_font(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let s = settings_entity(cx).read(cx).settings.clone();

        let ui_section_title = i18n!(cx, "settings.font.section_ui");
        let terminal_section_title = i18n!(cx, "settings.font.section_terminal");

        let ui_font_size_label = i18n!(cx, "settings.font.ui_size");
        let ui_scale_label = i18n!(cx, "settings.font.ui_scale");
        let terminal_font_size_label = i18n!(cx, "settings.font.terminal_size");
        let line_height_label = i18n!(cx, "settings.font.terminal_line_height");

        div()
            .flex()
            .flex_col()
            .gap(SPACE_XL)
            // 界面与阅读全局字体
            .child(
                div()
                    .child(section_header(&ui_section_title, &t, cx))
                    .child(
                        section_container(&t)
                            .child(self.render_number_stepper(
                                "ui-font-size",
                                &ui_font_size_label,
                                s.ui_font_size,
                                "{}",
                                8.0,
                                48.0,
                                0.5,
                                50.0,
                                true,
                                |state, val, cx| state.set_ui_font_size(val, cx),
                                window,
                                cx,
                            ))
                            .child(self.render_ui_font_dropdown_row(&s.ui_font_family, cx))
                            .child(self.render_number_stepper(
                                "ui-scale",
                                &ui_scale_label,
                                s.ui_scale,
                                "{}%",
                                50.0,
                                200.0,
                                5.0,
                                200.0,
                                false,
                                |state, val, cx| state.set_ui_scale(val, cx),
                                window,
                                cx,
                            )),
                    ),
            )
            // 终端与代码字体
            .child(
                div()
                    .child(section_header(&terminal_section_title, &t, cx))
                    .child(
                        section_container(&t)
                            .child(self.render_number_stepper(
                                "font-size",
                                &terminal_font_size_label,
                                s.font_size,
                                "{}",
                                6.0,
                                72.0,
                                0.5,
                                50.0,
                                true,
                                |state, val, cx| state.set_font_size(val, cx),
                                window,
                                cx,
                            ))
                            .child(self.render_font_dropdown_row(&s.font_family, cx))
                            .child(self.render_font_weight_dropdown_row(&s.font_weight, cx))
                            .child(self.render_number_stepper(
                                "line-height",
                                &line_height_label,
                                s.line_height,
                                "{}",
                                0.8,
                                3.0,
                                0.1,
                                50.0,
                                false,
                                |state, val, cx| state.set_line_height(val, cx),
                                window,
                                cx,
                            )),
                    ),
            )
    }
}
