use crate::settings::settings_entity;
use crate::theme::theme;
use crate::ui::tokens::ui_text_md;
use velowork_ui::input::{Input, InputState};
use crate::workspace::settings::FileSortBy;
use gpui::*;
use velowork_ui::tokens::{SPACE_SM, SPACE_MD, SPACE_LG};
use velowork_i18n::i18n;

use super::components::*;
use super::SettingsPanel;

impl SettingsPanel {
    pub(super) fn render_file_manager(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let s = settings_entity(cx).read(cx).settings.clone();

        let file_opener_label = i18n!(cx, "settings.file_opener");
        let file_opener_desc = i18n!(cx, "settings.file_opener_desc");
        let show_hidden_files_label = i18n!(cx, "settings.show_hidden_files");
        let _file_sort_by_label = i18n!(cx, "settings.file_sort_by.label");
        let alternating_row_bg_label = i18n!(cx, "settings.alternating_row_bg");
        let sftp_file_mode_label = i18n!(cx, "settings.sftp_default_file_mode");
        let sftp_dir_mode_label = i18n!(cx, "settings.sftp_default_dir_mode");

        div()
            .child(section_header(&file_opener_label, &t, cx))
            .child(
                section_container(&t)
                    .child(
                        div()
                            .px(SPACE_LG)
                            .py(SPACE_MD)
                            .flex()
                            .flex_col()
                            .gap(SPACE_SM)
                            .child(
                                div()
                                    .text_size(ui_text_md(cx))
                                    .text_color(rgb(t.text_muted))
                                    .child(file_opener_desc),
                            )
                            .child(
                                div().child(Input::new(&self.file_opener_input)),
                            ),
                    ),
            )
            .child(section_header(&i18n!(cx, "settings.nav.file_manager"), &t, cx))
            .child(
                section_container(&t)
                    .child(self.render_toggle(
                        "show-hidden-files", &show_hidden_files_label, s.show_hidden_files, true,
                        |state, val, cx| state.set_show_hidden_files(val, cx), cx,
                    ))
                    .child(self.render_file_sort_by_row(s.file_sort_by, cx))
                    .child(self.render_toggle(
                        "alternating-row-bg", &alternating_row_bg_label, s.alternating_row_bg, false,
                        |state, val, cx| state.set_alternating_row_bg(val, cx), cx,
                    ))
                    .child(self.render_mode_input_row(
                        "sftp-default-file-mode",
                        &sftp_file_mode_label,
                        &self.sftp_file_mode_input,
                        cx,
                    ))
                    .child(self.render_mode_input_row(
                        "sftp-default-dir-mode",
                        &sftp_dir_mode_label,
                        &self.sftp_dir_mode_input,
                        cx,
                    )),
            )
    }

    fn render_file_sort_by_row(
        &mut self,
        current: FileSortBy,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx);
        let label = i18n!(cx, "settings.file_sort_by.label");
        let focus_handle = self.get_or_create_radio_focus_handle("file-sort-by", cx);

        settings_row("file-sort-by".to_string(), &label, &t, cx, true).child(
            RadioGroup::new("file-sort-by-radio")
                .focus(&focus_handle)
                .mode(RadioMode::Button)
                .selected(Some(current))
                .options(
                    FileSortBy::all_variants()
                        .iter()
                        .map(|&sort| RadioOption::new(sort, i18n!(cx, sort.translation_key())))
                        .collect(),
                )
                .on_change(move |&sort, _, cx| {
                    settings_entity(cx).update(cx, |state, cx| {
                        state.set_file_sort_by(sort, cx);
                    });
                }),
        )
    }

    fn render_mode_input_row(
        &self,
        id: &str,
        label: &str,
        input: &Entity<InputState>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx);
        settings_row(id.to_string(), label, &t, cx, true)
            .child(
                div()
                    .w(px(120.0))
                    .child(Input::new(input)),
            )
    }
}
