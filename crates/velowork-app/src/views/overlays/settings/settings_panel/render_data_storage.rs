//! Data & Storage configuration: Data Root directory, logs, and terminal recordings.

use crate::keybindings::Cancel;
use crate::theme::theme;
use crate::ui::tokens::{mono_font_family, ui_text_md, ui_text_sm, ui_text_xs};
use crate::views::components::{modal_backdrop, modal_content, modal_header};
use gpui::*;
use gpui::prelude::*;
use std::path::PathBuf;
use velowork_core::data_root::{
    app_portable_data_root, persist_bootstrap_config, system_default_data_root, BootstrapConfig,
    DataRootMode,
};
use velowork_i18n::i18n;
use velowork_ui::button::Button;
use velowork_ui::h_flex;
use velowork_ui::icon::AppIcon;
use velowork_ui::select::Select;
use velowork_ui::tokens::{ICON_STD, RADIUS_MD, SPACE_2XS, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL, SPACE_XS};
use velowork_ui::v_flex;

use super::components::*;
use super::SettingsPanel;

impl SettingsPanel {
    pub(super) fn render_data_storage(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);
        let current_dr = velowork_core::data_root::current();
        let selected_mode = self.data_root_mode_select.read(cx).selected_value().copied().unwrap_or_default();
        let is_custom = selected_mode == DataRootMode::Custom;
        let error = self.data_root_error.clone();

        // Calculate whether current selection matches active running data root
        let custom_input_val = self.data_root_custom_input.read(cx).value(cx).trim().to_string();
        let is_currently_active = match selected_mode {
            DataRootMode::Default => current_dr.mode() == DataRootMode::Default,
            DataRootMode::AppDir => current_dr.mode() == DataRootMode::AppDir,
            DataRootMode::Custom => {
                current_dr.mode() == DataRootMode::Custom
                    && !custom_input_val.is_empty()
                    && std::path::Path::new(&custom_input_val) == current_dr.root()
            }
        };

        // --- Mode Select Row ---
        let mode_row = settings_row_with_desc(
            "data-root-mode",
            &i18n!(cx, "settings.data_storage.mode"),
            &i18n!(cx, "settings.data_storage.mode_desc"),
            &t,
            cx,
            false,
        )
        .child(
            div()
                .w(px(240.0))
                .child(Select::new(&self.data_root_mode_select)),
        );

        // --- Custom Path Input Row (only when Custom mode selected) ---
        let bounds_setter = Self::bounds_setter(cx, |s, b| s.data_root_input_bounds = b);
        let custom_path_row = settings_row_with_desc(
            "data-root-custom-path",
            &i18n!(cx, "settings.data_storage.custom_path"),
            &i18n!(cx, "settings.data_storage.custom_path_desc"),
            &t,
            cx,
            false,
        )
        .child(
            h_flex()
                .items_center()
                .gap(SPACE_SM)
                .child(
                    div()
                        .relative()
                        .w(px(280.0))
                        .child(self.data_root_custom_input.clone())
                        .child(canvas(bounds_setter, |_, _, _, _| {}).absolute().inset_0()),
                )
                .child(self.render_data_root_picker_button(cx)),
        );

        // --- Inline validation error ---
        let error_row = if let Some(ref err) = error {
            h_flex()
                .w_full()
                .justify_end()
                .items_center()
                .gap(SPACE_XS)
                .px(SPACE_XS)
                .py(SPACE_2XS)
                .child(
                    AppIcon::Ban
                        .size(ui_text_sm(cx))
                        .text_color(rgb(t.error)),
                )
                .child(
                    div()
                        .text_size(ui_text_sm(cx))
                        .text_color(rgb(t.error))
                        .child(err.clone()),
                )
                .into_any_element()
        } else {
            div().into_any_element()
        };

        // --- Action / Status Row ---
        let action_row = h_flex()
            .w_full()
            .justify_end()
            .items_center()
            .gap(SPACE_MD)
            .py(SPACE_SM)
            .child(if is_currently_active {
                h_flex()
                    .items_center()
                    .gap(SPACE_XS)
                    .child(
                        AppIcon::Check
                            .size(ICON_STD)
                            .text_color(rgb(t.success)),
                    )
                    .child(
                        div()
                            .text_size(ui_text_sm(cx))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(t.success))
                            .child(i18n!(cx, "settings.data_storage.current_active")),
                    )
                    .into_any_element()
            } else {
                h_flex()
                    .items_center()
                    .gap(SPACE_MD)
                    .child(
                        div()
                            .text_size(ui_text_sm(cx))
                            .text_color(rgb(t.text_muted))
                            .child(i18n!(cx, "settings.data_storage.restart_hint")),
                    )
                    .child({
                        let apply_fh = self.get_or_create_button_focus_handle("apply-data-root-btn", cx);
                        Button::new("apply-data-root-btn", &t)
                            .label(i18n!(cx, "settings.data_storage.apply"))
                            .primary()
                            .icon_left(AppIcon::Check)
                            .focus_handle(&apply_fh)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.trigger_data_root_apply(cx);
                            }))
                    })
                    .into_any_element()
            });

        // --- Overview card of active paths ---
        let active_root_str = current_dr.root().to_string_lossy().to_string();
        let active_logs_str = current_dr.logs_dir().to_string_lossy().to_string();
        let active_rec_str = current_dr.recordings_dir().to_string_lossy().to_string();

        let overview_section = v_flex()
            .w_full()
            .gap(SPACE_SM)
            .pt(SPACE_MD)
            .child(
                h_flex()
                    .items_center()
                    .gap(SPACE_SM)
                    .pb(SPACE_XS)
                    .child(
                        AppIcon::Folder
                            .size(ICON_STD)
                            .text_color(rgb(t.border_active)),
                    )
                    .child(
                        div()
                            .text_size(ui_text_md(cx))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(t.text_primary))
                            .child(i18n!(cx, "settings.data_storage.resolved_title")),
                    ),
            )
            .child(
                v_flex()
                    .w_full()
                    .border_1()
                    .border_color(p.border_subtle)
                    .rounded(RADIUS_MD)
                    .bg(p.surface_card)
                    .overflow_hidden()
                    .child(self.render_path_preview_row(
                        "data-root",
                        &i18n!(cx, "settings.data_storage.path_data_root"),
                        &active_root_str,
                        current_dr.root().to_path_buf(),
                        true,
                        cx,
                    ))
                    .child(self.render_path_preview_row(
                        "app-logs",
                        &i18n!(cx, "settings.data_storage.path_app_logs"),
                        &active_logs_str,
                        current_dr.logs_dir(),
                        true,
                        cx,
                    ))
                    .child(self.render_path_preview_row(
                        "recordings",
                        &i18n!(cx, "settings.data_storage.path_recordings"),
                        &active_rec_str,
                        current_dr.recordings_dir(),
                        false,
                        cx,
                    )),
            );

        let config_section = section_container(&t)
            .child(mode_row)
            .when(is_custom, |d| d.child(custom_path_row))
            .when(is_custom && error.is_some(), |d| d.child(error_row))
            .child(action_row);

        let main_content = v_flex()
            .w_full()
            .gap(SPACE_MD)
            .child(
                div()
                    .pb(SPACE_XS)
                    .text_size(ui_text_sm(cx))
                    .text_color(rgb(t.text_muted))
                    .child(i18n!(cx, "settings.data_storage.description")),
            )
            .child(config_section)
            .child(overview_section);

        div()
            .w_full()
            .relative()
            .child(main_content)
            .when(self.show_data_root_confirm_modal, |d| {
                d.child(self.render_migration_modal(cx))
            })
    }

    fn render_path_preview_row(
        &self,
        id_suffix: &str,
        label: &str,
        path_str: &str,
        path_buf: PathBuf,
        has_divider: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx);
        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);
        let open_label = i18n!(cx, "settings.data_storage.open_folder");

        h_flex()
            .w_full()
            .px(SPACE_LG)
            .py(SPACE_MD)
            .items_center()
            .justify_between()
            .gap(SPACE_MD)
            .when(has_divider, |d| d.border_b_1().border_color(p.border_subtle))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(SPACE_2XS)
                    .child(
                        div()
                            .text_size(ui_text_xs(cx))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(t.text_muted))
                            .child(label.to_string()),
                    )
                    .child(
                        div()
                            .text_size(ui_text_sm(cx))
                            .font_family(mono_font_family(cx))
                            .text_color(rgb(t.text_primary))
                            .overflow_hidden()
                            .text_ellipsis()
                            .child(path_str.to_string()),
                    ),
            )
            .child(
                Button::new(ElementId::Name(format!("open-dir-{}", id_suffix).into()), &t)
                    .icon_left(AppIcon::FolderOpen)
                    .tooltip(open_label)
                    .on_click(cx.listener(move |_, _, _, _| {
                        velowork_core::process::open_in_file_manager(&path_buf);
                    })),
            )
    }

    fn render_data_root_picker_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let label = i18n!(cx, "settings.data_storage.browse");
        let browse_fh = self.get_or_create_button_focus_handle("data-root-browse-btn", cx);
        Button::new("data-root-browse-btn", &t)
            .icon_left(AppIcon::Folder)
            .tooltip(label)
            .focus_handle(&browse_fh)
            .on_click(cx.listener(|this, _, window, cx| {
                this.open_data_root_picker(window, cx);
            }))
    }

    pub(super) fn open_data_root_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let prompt = i18n!(cx, "settings.data_storage.browse_title");
        let paths = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(prompt.into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(selected))) = paths.await
                && let Some(path) = selected.first().cloned()
            {
                let p = path.to_string_lossy().to_string();
                let _ = this.update(cx, |this, cx| {
                    this.data_root_custom_input
                        .update(cx, |st, cx| st.set_value_quiet(p, cx));
                    this.recompute_data_root_error(cx);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub(super) fn render_data_root_suggestions(&self, cx: &mut Context<Self>) -> impl IntoElement {
        velowork_ui::render_path_suggestions(
            "data-root-suggestions",
            &self.data_root_custom_input,
            cx,
        )
    }

    pub(super) fn recompute_data_root_error(&mut self, cx: &App) {
        let raw = self.data_root_custom_input.read(cx).value(cx);
        self.data_root_error = Self::validate_data_root_path(&raw, cx);
    }

    pub(crate) fn validate_data_root_path(raw: &str, cx: &App) -> Option<String> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Some(i18n!(cx, "settings.data_storage.error_invalid"));
        }
        let path = PathBuf::from(raw);
        if !path.is_absolute() {
            return Some(i18n!(cx, "settings.data_storage.error_not_absolute"));
        }
        if path.exists() {
            if !path.is_dir() {
                return Some(i18n!(cx, "settings.data_storage.error_not_dir"));
            }
            if !velowork_core::data_root::is_dir_empty(&path) {
                return Some(i18n!(cx, "settings.data_storage.error_not_empty"));
            }
            if !dir_writable(&path) {
                return Some(i18n!(cx, "settings.data_storage.error_not_writable"));
            }
            return None;
        }
        match path.parent() {
            Some(parent) if parent.exists() && parent.is_dir() => {
                if dir_writable(parent) {
                    None
                } else {
                    Some(i18n!(cx, "settings.data_storage.error_parent_not_writable"))
                }
            }
            _ => Some(i18n!(cx, "settings.data_storage.error_parent_missing")),
        }
    }

    fn trigger_data_root_apply(&mut self, cx: &mut Context<Self>) {
        let selected_mode = self.data_root_mode_select.read(cx).selected_value().copied().unwrap_or_default();
        if selected_mode == DataRootMode::Custom {
            self.recompute_data_root_error(cx);
            if self.data_root_error.is_some() {
                cx.notify();
                return;
            }
        }
        self.show_data_root_confirm_modal = true;
        cx.notify();
    }

    fn render_migration_modal(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);
        let selected_mode = self.data_root_mode_select.read(cx).selected_value().copied().unwrap_or_default();
        let target_path = match selected_mode {
            DataRootMode::Default => system_default_data_root(),
            DataRootMode::AppDir => app_portable_data_root(),
            DataRootMode::Custom => {
                let val = self.data_root_custom_input.read(cx).value(cx).trim().to_string();
                PathBuf::from(val)
            }
        };
        let target_path_str = target_path.to_string_lossy().to_string();
        let title = i18n!(cx, "settings.data_storage.migration_title");
        let desc = i18n!(cx, "settings.data_storage.migration_desc").replace("{path}", &target_path_str);

        modal_backdrop("data-root-modal-backdrop", &t, cx)
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                this.show_data_root_confirm_modal = false;
                cx.notify();
            }))
            .child(
                modal_content("data-root-modal-content", cx)
                    .w(px(520.0))
                    .on_action(cx.listener(|this, _: &Cancel, _, cx| {
                        this.show_data_root_confirm_modal = false;
                        cx.notify();
                    }))
                .child(modal_header(
                    title,
                    Option::<String>::None,
                    &t,
                    cx,
                    cx.listener(|this, _, _, cx| {
                        this.show_data_root_confirm_modal = false;
                        cx.notify();
                    }),
                ))
                .child(
                    v_flex()
                        .p(SPACE_XL)
                        .child(
                            div()
                                .text_size(ui_text_md(cx))
                                .text_color(rgb(t.text_primary))
                                .child(desc),
                        ),
                )
                .child(
                    h_flex()
                        .h(px(52.0))
                        .flex_shrink_0()
                        .w_full()
                        .px(SPACE_LG)
                        .border_t_1()
                        .border_color(p.border_subtle)
                        .items_center()
                        .justify_end()
                        .gap(SPACE_MD)
                        .child(
                            Button::new("cancel-migration-btn", &t)
                                .label(i18n!(cx, "common.cancel"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.show_data_root_confirm_modal = false;
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("clean-migration-btn", &t)
                                .label(i18n!(cx, "settings.data_storage.migration_clean_restart"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.execute_data_root_switch(false, cx);
                                })),
                        )
                        .child(
                            Button::new("copy-migration-btn", &t)
                                .label(i18n!(cx, "settings.data_storage.migration_copy_restart"))
                                .primary()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.execute_data_root_switch(true, cx);
                                })),
                        ),
                ),
        )
    }

    fn execute_data_root_switch(&mut self, copy_data: bool, cx: &mut Context<Self>) {
        self.show_data_root_confirm_modal = false;
        let selected_mode = self.data_root_mode_select.read(cx).selected_value().copied().unwrap_or_default();
        let target_path = match selected_mode {
            DataRootMode::Default => system_default_data_root(),
            DataRootMode::AppDir => app_portable_data_root(),
            DataRootMode::Custom => {
                let val = self.data_root_custom_input.read(cx).value(cx).trim().to_string();
                PathBuf::from(val)
            }
        };

        let current_root = velowork_core::data_root::current().root();

        if copy_data && current_root != target_path {
            if let Err(e) = velowork_core::data_root::migrate_data_root(current_root, &target_path) {
                crate::views::panels::toast::ToastManager::error(
                    format!("{}: {}", i18n!(cx, "settings.data_storage.copy_failed"), e),
                    cx,
                );
                return;
            }
            crate::views::panels::toast::ToastManager::info(
                i18n!(cx, "settings.data_storage.copy_success"),
                cx,
            );
        }

        let config = BootstrapConfig {
            mode: selected_mode,
            custom_path: if selected_mode == DataRootMode::Custom {
                Some(target_path)
            } else {
                None
            },
        };

        if let Err(e) = persist_bootstrap_config(&config) {
            crate::views::panels::toast::ToastManager::error(
                format!("Failed to save bootstrap configuration: {e}"),
                cx,
            );
            return;
        }

        let toast = velowork_workspace::toast::Toast::warning(
            i18n!(cx, "settings.data_storage.restart_notice"),
        )
        .with_actions(vec![velowork_workspace::toast::ToastAction::new(
            "restart_app",
            i18n!(cx, "update.restart"),
            velowork_workspace::toast::ToastActionStyle::Primary,
        )]);
        velowork_workspace::toast::ToastManager::post(toast, cx);
        cx.notify();
    }
}

fn dir_writable(dir: &std::path::Path) -> bool {
    use std::fs::OpenOptions;
    let probe = dir.join(format!(".velowork_write_test_{}.tmp", std::process::id()));
    match OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&probe)
    {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}
