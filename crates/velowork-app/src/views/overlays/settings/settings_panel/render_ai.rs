use crate::keybindings::Cancel;
use crate::settings::settings_entity;
use crate::theme::theme;
use crate::ui::tokens::{ui_text_md, ui_text_sm, SPACE_XS, SPACE_SM, SPACE_MD, SPACE_LG, SPACE_XL, RADIUS_LG, dialog_md};
use crate::views::components::{modal_backdrop, modal_content, modal_header};
use velowork_ui::dialog_actions::{dialog_actions, focusable_action_button};
use velowork_ui::focus_group::{FocusGroup, FocusGroupExt};
use velowork_ui::input::{Input, InputState};
use velowork_ui::select::{Select, SelectOption};
use crate::workspace::settings::AiModelConfig;
use gpui::*;
use gpui::prelude::*;
use velowork_ui::v_flex;
use velowork_ui::h_flex;
use velowork_ui::button;
use velowork_ui::button_primary;
use velowork_ui::ControlSize;
use velowork_ui::Tooltip;
use velowork_i18n::i18n;

use super::components::*;
use super::SettingsPanel;

impl SettingsPanel {
    pub(super) fn render_ai(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let s = settings_entity(cx).read(cx).settings.clone();

        let title = i18n!(cx, "settings.nav.ai_assistant");
        let enable_label = i18n!(cx, "settings.ai_assistant.enable");
        let enable_desc = i18n!(cx, "settings.ai_assistant.enable_desc");
        let model_mgmt_label = i18n!(cx, "settings.ai_assistant.model_management");
        let add_model_label = i18n!(cx, "settings.ai_assistant.add_model");
        let default_config_label = i18n!(cx, "settings.ai_assistant.default_config");
        let default_model_label = i18n!(cx, "settings.ai_assistant.default_model");
        let default_model_desc = i18n!(cx, "settings.ai_assistant.default_model_desc");
        let temperature_label = i18n!(cx, "settings.ai_assistant.temperature");
        let temperature_desc = i18n!(cx, "settings.ai_assistant.temperature_desc");
        let max_tokens_label = i18n!(cx, "settings.ai_assistant.max_tokens");
        let max_tokens_desc = i18n!(cx, "settings.ai_assistant.max_tokens_desc");

        let models = s.ai_models.clone();
        let ai_enabled = s.ai_enabled;
        let default_model_id = s.ai_default_model_id.clone();
        let all_models = s.ai_models.clone();
        let _temperature = s.ai_temperature;
        let _max_tokens = s.ai_max_tokens;

        div()
            .relative()
            .child(section_header(&title, &t, cx))
            // Enable toggle
            .child(
                section_container(&t).child(self.render_toggle_with_desc(
                    "ai-enable",
                    &enable_label,
                    &enable_desc,
                    ai_enabled,
                    false,
                    |state, val, cx| state.set_ai_enabled(val, cx),
                    cx,
                )),
            )
            // Model management section
            .when(ai_enabled, |d| {
                d.child(section_header(&model_mgmt_label, &t, cx))
                    .child({
                        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);
                        let section = v_flex()
                            .mx(SPACE_LG)
                            .rounded(RADIUS_LG)
                            .border_1()
                            .border_color(p.border_subtle)
                            .bg(p.surface_card)
                            .overflow_hidden();
                        let mut section = section;
                        for (idx, model) in models.iter().enumerate() {
                            let is_last = idx == models.len() - 1;
                            section = section.child(self.render_ai_model_item(model, idx, is_last, &t, cx));
                        }
                        section
                    })
                    // Add model button (outside the table)
                    .child(
                        div()
                            .mx(SPACE_LG)
                            .my(SPACE_MD)
                            .child({
                                let add_btn_fh = self.get_or_create_button_focus_handle("ai-add-model-btn", cx);
                                button_primary("ai-add-model-btn", add_model_label.clone(), &t)
                                    .full_width(true)
                                    .focus_handle(&add_btn_fh)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_add_model_dialog(window, cx);
                                    }))
                            }),
                    )
                    // Skill management section
                    .child(section_header(&i18n!(cx, "settings.ai_assistant.skill_management"), &t, cx))
                    .child({
                        let mut section = section_container(&t);
                        let builtin_skills = velowork_ai::SkillRegistry::new(velowork_ai::builtin_skills()).metas();
                        for meta in builtin_skills {
                            let skill_name = meta.name.clone();
                            let is_enabled = s.is_ai_skill_enabled(&skill_name);
                            let label = i18n!(cx, &meta.i18n_key);
                            let desc = i18n!(cx, meta.tier.label_key());

                            section = section.child(
                                settings_row_with_desc(
                                    format!("skill-row-{}", skill_name),
                                    &label,
                                    &desc,
                                    &t,
                                    cx,
                                    false,
                                )
                                .child({
                                    let fh = self.get_or_create_toggle_focus_handle(&format!("skill-toggle-{}", skill_name), cx);
                                    velowork_ui::Switch::new(format!("skill-toggle-{}", skill_name))
                                        .focus(&fh)
                                        .checked(is_enabled)
                                        .on_click(cx.listener(move |_, checked: &bool, _, cx| {
                                            let val = *checked;
                                            let name = skill_name.clone();
                                            settings_entity(cx).update(cx, |state, cx| {
                                                state.set_ai_skill_enabled(&name, val, cx);
                                            });
                                        }))
                                }),
                            );
                        }
                        section
                    })
                    // Default config section
                    .child(section_header(&default_config_label, &t, cx))
                    .child({
                        let _current_label = default_model_id
                            .as_ref()
                            .and_then(|id| {
                                all_models.iter().find(|m| &m.id == id).map(|m| {
                                    format!("{} - {}", m.name, m.model_id)
                                })
                            })
                            .unwrap_or_else(|| "None".to_string());

                        // Build section without overflow_hidden so dropdown can escape
                        let section = section_container(&t)
                            // Default model selector row
                            .child(
                                settings_row_with_desc(
                                    "ai-default-model",
                                    &default_model_label,
                                    &default_model_desc,
                                    &t,
                                    cx,
                                    true,
                                )
                                .child(div().w(px(200.0)).child(Select::new(&self.ai_default_model_select)))
                            )
                            // Temperature
                            .child({
                                settings_row_with_desc(
                                    "ai-temperature",
                                    &temperature_label,
                                    &temperature_desc,
                                    &t,
                                    cx,
                                    true,
                                )
                                .child(
                                    div()
                                        .w(px(80.0))
                                        .child(Input::new(&self.ai_temperature_input)),
                                )
                            })
                            // Max Tokens
                            .child({
                                settings_row_with_desc(
                                    "ai-max-tokens",
                                    &max_tokens_label,
                                    &max_tokens_desc,
                                    &t,
                                    cx,
                                    false,
                                )
                                .child(
                                    div()
                                        .w(px(80.0))
                                        .child(Input::new(&self.ai_max_tokens_input)),
                                )
                            });
                        section
                    })
                    // Context and Memory section
                    .child(section_header(&i18n!(cx, "settings.ai_assistant.context_and_memory"), &t, cx))
                    .child({
                        section_container(&t)
                            // Auto Compress toggle
                            .child(self.render_toggle_with_desc(
                                "ai-auto-compress",
                                &i18n!(cx, "settings.ai_assistant.auto_compress"),
                                &i18n!(cx, "settings.ai_assistant.auto_compress_desc"),
                                s.ai_auto_compress,
                                true,
                                |state, val, cx| state.set_ai_auto_compress(val, cx),
                                cx,
                            ))
                            // Compression Strategy
                            .child(
                                settings_row_with_desc(
                                    "ai-compression-strategy",
                                    &i18n!(cx, "settings.ai_assistant.compression_strategy"),
                                    &i18n!(cx, "settings.ai_assistant.compression_strategy_desc"),
                                    &t,
                                    cx,
                                    true,
                                )
                                .child(div().w(px(200.0)).child(Select::new(&self.ai_compression_strategy_select))),
                            )
                            // Max Context Tokens
                            .child(
                                settings_row_with_desc(
                                    "ai-max-context-tokens",
                                    &i18n!(cx, "settings.ai_assistant.max_context_tokens"),
                                    &i18n!(cx, "settings.ai_assistant.max_context_tokens_desc"),
                                    &t,
                                    cx,
                                    true,
                                )
                                .child(
                                    div()
                                        .w(px(100.0))
                                        .child(Input::new(&self.ai_max_context_tokens_input)),
                                )
                            )
                            // Max History Messages
                            .child(
                                settings_row_with_desc(
                                    "ai-max-history-messages",
                                    &i18n!(cx, "settings.ai_assistant.max_history_messages"),
                                    &i18n!(cx, "settings.ai_assistant.max_history_messages_desc"),
                                    &t,
                                    cx,
                                    false,
                                )
                                .child(
                                    div()
                                        .w(px(80.0))
                                        .child(Input::new(&self.ai_max_history_messages_input)),
                                )
                            )
                    })
                    // Terminal Interaction section
                    .child(section_header(&i18n!(cx, "settings.ai_assistant.terminal_interaction"), &t, cx))
                    .child({
                        section_container(&t).child(self.render_toggle_with_desc(
                            "terminal-ai-floating-toolbar",
                            &i18n!(cx, "settings.ai_assistant.floating_toolbar"),
                            &i18n!(cx, "settings.ai_assistant.floating_toolbar_desc"),
                            s.terminal_ai_floating_toolbar_enabled,
                            false,
                            |state, val, cx| state.set_terminal_ai_floating_toolbar_enabled(val, cx),
                            cx,
                        ))
                    })
            })
    }

    fn render_ai_model_item(
        &self,
        model: &AiModelConfig,
        _index: usize,
        is_last: bool,
        t: &velowork_core::theme::ThemeColors,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let edit_label = i18n!(cx, "common.edit");
        let delete_label = i18n!(cx, "common.delete");
        let detail = format!("{} · {}", model.base_url, model.model_id);
        let model_id = model.id.clone();
        let model_id_for_delete = model_id.clone();

        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);
        div()
            .id(ElementId::Name(format!("ai-model-item-{}", model.id).into()))
            .px(SPACE_LG)
            .py(SPACE_MD)
            .when(!is_last, |d| {
                d.border_b_1().border_color(p.border_subtle)
            })
            .flex()
            .items_center()
            .gap(SPACE_LG)
            // Model info
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(2.0))
                    .child(
                        div()
                            .text_size(ui_text_md(cx))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(t.text_primary))
                            .child(model.name.clone()),
                    )
                    .child(
                        div()
                            .text_size(ui_text_sm(cx))
                            .text_color(rgb(t.text_muted))
                            .font_family("monospace")
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(detail),
                    ),
            )
            // Actions
            .child({
                let model_id_clone = model_id.clone();
                let toggle_fh = self.get_or_create_toggle_focus_handle(&format!("ai-model-toggle-{}", model.id), cx);
                let edit_fh = self.get_or_create_button_focus_handle(&format!("ai-model-edit-{}", model.id), cx);
                let delete_fh = self.get_or_create_button_focus_handle(&format!("ai-model-delete-{}", model.id), cx);
                div()
                    .flex()
                    .items_center()
                    .gap(SPACE_SM)
                    // Toggle switch
                    .child({
                        let mid = model_id_clone.clone();
                        velowork_ui::Switch::new(format!("ai-model-toggle-{}", model.id))
                            .focus(&toggle_fh)
                            .checked(model.enabled)
                            .on_click(cx.listener(move |_, checked: &bool, _, cx| {
                                let val = *checked;
                                settings_entity(cx).update(cx, |state, cx| {
                                    state.toggle_ai_model(&mid, val, cx);
                                });
                            }))
                    })
                    // Edit button
                    .child(
                        button(
                            ElementId::Name(format!("ai-model-edit-{}", model.id).into()),
                            edit_label.clone(),
                            &t,
                        )
                        .size(ControlSize::Compact)
                        .focus_handle(&edit_fh)
                        .on_click({
                            let mid = model_id_clone.clone();
                            cx.listener(move |this, _, window, cx| {
                                this.open_edit_model_dialog(&mid, window, cx);
                            })
                        }),
                    )
                    // Delete button
                    .child(
                        button(
                            ElementId::Name(format!("ai-model-delete-{}", model.id).into()),
                            delete_label.clone(),
                            &t,
                        )
                        .size(ControlSize::Compact)
                        .danger(true)
                        .focus_handle(&delete_fh)
                        .on_click({
                            let mid = model_id_for_delete.clone();
                            let select = self.ai_default_model_select.clone();
                            cx.listener(move |_, _, _, cx| {
                                settings_entity(cx).update(cx, |state, cx| {
                                    state.remove_ai_model(&mid, cx);
                                });
                                let models = settings_entity(cx).read(cx).settings.ai_models.clone();
                                let selected = select.read(cx).selected_value().cloned();
                                let options = models
                                    .iter()
                                    .map(|m| {
                                        SelectOption::new(
                                            m.id.clone(),
                                            format!("{} ({})", m.name, m.model_id),
                                        )
                                    })
                                    .collect();
                                select.update(cx, |s, cx| {
                                    s.set_options(options, cx);
                                    s.set_selected_value(selected, cx);
                                });
                            })
                        }),
                    )
            })
    }

    pub(super) fn render_add_model_dialog(
        &self,
        t: &velowork_core::theme::ThemeColors,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let title = if self.ai_edit_model_id.is_some() {
            i18n!(cx, "settings.ai_assistant.edit_model_title")
        } else {
            i18n!(cx, "settings.ai_assistant.add_model_title")
        };
        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);
        let name_label = i18n!(cx, "settings.ai_assistant.model_name");
        let base_url_label = i18n!(cx, "settings.ai_assistant.base_url");
        let api_key_label = i18n!(cx, "settings.ai_assistant.api_key");
        let model_id_label = i18n!(cx, "settings.ai_assistant.model_id");
        let desc_label = i18n!(cx, "settings.ai_assistant.description");
        let cancel_label = i18n!(cx, "common.cancel");
        let save_label = i18n!(cx, "common.save");
        let test_label = i18n!(cx, "common.test");
        let test_testing_label = i18n!(cx, "settings.ai_assistant.test_testing");
        let _test_success_label = i18n!(cx, "status.test_success");
        let test_in_progress = self.ai_test_in_progress;
        let test_result = self.ai_test_result.clone();
        let focus_group = FocusGroup::new();
        focus_group.extend([
            self.ai_model_name_input.read(cx).focus_handle(cx),
            self.ai_model_base_url_input.read(cx).focus_handle(cx),
            self.ai_model_api_key_input.read(cx).focus_handle(cx),
            self.ai_model_id_input.read(cx).focus_handle(cx),
            self.ai_model_desc_input.read(cx).focus_handle(cx),
            self.ai_dialog_test_focus.clone(),
            self.ai_dialog_cancel_focus.clone(),
            self.ai_dialog_save_focus.clone(),
        ]);

        // Backdrop
        let _weak = cx.weak_entity();
        modal_backdrop("ai-model-dialog-backdrop", &t, cx)
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| {
                this.close_add_model_dialog(Some(window), cx);
            }))
            .tab_cycle(&focus_group)
            .child(
                modal_content("ai-model-dialog", cx)
                    .w(dialog_md(cx))
                    .max_h(px(620.0))
                    .on_action(cx.listener(|this, _: &Cancel, window, cx| {
                        this.close_add_model_dialog(Some(window), cx);
                    }))
                    .child(modal_header(
                        title,
                        Option::<String>::None,
                        &t,
                        cx,
                        cx.listener(|this, _, window, cx| this.close_add_model_dialog(Some(window), cx)),
                    ))
                    .child(
                        v_flex()
                            .p(SPACE_LG)
                            .gap(SPACE_XL)
                            // Model Name
                            .child(self.render_dialog_input(
                                "dialog-model-name",
                                &name_label,
                                &self.ai_model_name_input,
                                t,
                                cx,
                            ))
                            // Base URL
                            .child(self.render_dialog_input(
                                "dialog-model-base-url",
                                &base_url_label,
                                &self.ai_model_base_url_input,
                                t,
                                cx,
                            ))
                            // API Key
                            .child(self.render_dialog_password_input(
                                "dialog-model-api-key",
                                &api_key_label,
                                &self.ai_model_api_key_input,
                                t,
                                cx,
                            ))
                            // Model ID
                            .child(self.render_dialog_input(
                                "dialog-model-model-id",
                                &model_id_label,
                                &self.ai_model_id_input,
                                t,
                                cx,
                            ))
                            // Description
                            .child(self.render_dialog_input(
                                "dialog-model-desc",
                                &desc_label,
                                &self.ai_model_desc_input,
                                t,
                                cx,
                            )),
                    )
                    .child({
                        let test_status_text = match (&test_result, test_in_progress) {
                            (Some(Ok(msg)), _) => Some((msg.clone(), true)),
                            (Some(Err(msg)), _) => Some((msg.clone(), false)),
                            (None, true) => Some((test_testing_label.clone(), false)),
                            _ => None,
                        };

                        let test_label_for_btn =
                            if test_in_progress { test_testing_label } else { test_label };
                        let weak = cx.weak_entity();
                        let weak_test = weak.clone();
                        let weak_cancel = weak.clone();
                        let weak_save = weak.clone();

                        h_flex()
                            .h(px(52.0))
                            .flex_shrink_0()
                            .px(SPACE_LG)
                            .border_t_1()
                            .border_color(p.border_subtle)
                            .items_center()
                            .justify_between()
                            .gap(SPACE_MD)
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap(SPACE_MD)
                                    .flex_1()
                                    .min_w_0()
                                    .child(focusable_action_button(
                                        "dialog-test-btn",
                                        test_label_for_btn,
                                        move |_, _, cx| {
                                            weak_test.update(cx, |this, cx| {
                                                if !this.ai_test_in_progress {
                                                    this.test_model_from_dialog(cx);
                                                }
                                            })
                                            .ok();
                                        },
                                        &self.ai_dialog_test_focus,
                                        &t,
                                    ))
                                    // Test result text (truncate on overflow with full hover tooltip)
                                    .children(test_status_text.map(|(msg, is_success)| {
                                        let tip_msg = msg.clone();
                                        div()
                                            .id("dialog-test-result-text")
                                            .flex_1()
                                            .min_w_0()
                                            .overflow_hidden()
                                            .truncate()
                                            .text_size(ui_text_sm(cx))
                                            .text_color(rgb(if is_success { t.success } else { t.error }))
                                            .child(msg)
                                            .when(!test_in_progress, |d| {
                                                d.tooltip(move |_, cx| {
                                                    cx.new(|_| Tooltip::new(tip_msg.clone())).into()
                                                })
                                            })
                                    }))
                            )
                            // Cancel + Save (flex_shrink_0 prevents being squeezed)
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .child(
                                        dialog_actions(
                                            cancel_label,
                                            move |_, window, cx| {
                                                weak_cancel.update(cx, |this, cx| {
                                                    this.close_add_model_dialog(Some(window), cx)
                                                })
                                                .ok();
                                            },
                                            save_label,
                                            move |_, window, cx| {
                                                weak_save.update(cx, |this, cx| {
                                                    this.save_model_from_dialog(Some(window), cx)
                                                })
                                                .ok();
                                            },
                                            &self.ai_dialog_cancel_focus,
                                            &self.ai_dialog_save_focus,
                                            &t,
                                        ),
                                    )
                            )
                    }),
            )
    }

    pub(super) fn render_dialog_input(
        &self,
        id: &str,
        label: &str,
        input: &Entity<InputState>,
        t: &velowork_core::theme::ThemeColors,
        cx: &App,
    ) -> impl IntoElement {
        div()
            .id(ElementId::Name(id.into()))
            .flex()
            .flex_col()
            .gap(SPACE_XS)
            .child(
                div()
                    .text_size(ui_text_sm(cx))
                    .text_color(rgb(t.text_muted))
                    .child(label.to_string()),
            )
            .child(Input::new(input))
    }

    pub(super) fn render_dialog_password_input(
        &self,
        id: &str,
        label: &str,
        input: &Entity<InputState>,
        t: &velowork_core::theme::ThemeColors,
        cx: &App,
    ) -> impl IntoElement {
        div()
            .id(ElementId::Name(id.into()))
            .flex()
            .flex_col()
            .gap(SPACE_XS)
            .child(
                div()
                    .text_size(ui_text_sm(cx))
                    .text_color(rgb(t.text_muted))
                    .child(label.to_string()),
            )
            .child(Input::new(input).mask_toggle(true))
    }

    fn open_add_model_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dialog_trigger_focus_handle = Some(self.get_or_create_button_focus_handle("ai-add-model-btn", cx));
        self.ai_add_model_dialog_open = true;
        self.ai_edit_model_id = None;
        // Clear inputs
        self.ai_model_name_input
            .update(cx, |s, cx| s.set_value("", cx));
        self.ai_model_base_url_input
            .update(cx, |s, cx| s.set_value("", cx));
        self.ai_model_api_key_input
            .update(cx, |s, cx| s.set_value("", cx));
        self.ai_model_id_input
            .update(cx, |s, cx| s.set_value("", cx));
        self.ai_model_desc_input
            .update(cx, |s, cx| s.set_value("", cx));
        self.ai_model_name_input.update(cx, |s, cx| s.focus(window, cx));
        cx.notify();
    }

    fn open_edit_model_dialog(&mut self, model_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let s = settings_entity(cx).read(cx).settings.clone();
        if let Some(model) = s.ai_models.iter().find(|m| m.id == model_id) {
            self.dialog_trigger_focus_handle = Some(self.get_or_create_button_focus_handle(&format!("ai-model-edit-{}", model_id), cx));
            self.ai_add_model_dialog_open = true;
            self.ai_edit_model_id = Some(model_id.to_string());
            self.ai_model_name_input
                .update(cx, |s, cx| s.set_value(model.name.clone(), cx));
            self.ai_model_base_url_input
                .update(cx, |s, cx| s.set_value(model.base_url.clone(), cx));
            self.ai_model_api_key_input
                .update(cx, |s, cx| s.set_value(model.api_key.clone(), cx));
            self.ai_model_id_input
                .update(cx, |s, cx| s.set_value(model.model_id.clone(), cx));
            self.ai_model_desc_input
                .update(cx, |s, cx| s.set_value(model.description.clone(), cx));
            self.ai_model_name_input.update(cx, |s, cx| s.focus(window, cx));
            cx.notify();
        }
    }

    pub(super) fn close_add_model_dialog(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        self.ai_add_model_dialog_open = false;
        self.ai_edit_model_id = None;
        if let Some(window) = window
            && let Some(fh) = self.dialog_trigger_focus_handle.take()
        {
            window.focus(&fh, cx);
        }
        cx.notify();
    }

    fn save_model_from_dialog(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        let name = self.ai_model_name_input.read(cx).text().to_string();
        let base_url = self.ai_model_base_url_input.read(cx).text().to_string();
        let api_key = self.ai_model_api_key_input.read(cx).text().to_string();
        let model_id = self.ai_model_id_input.read(cx).text().to_string();
        let desc = self.ai_model_desc_input.read(cx).text().to_string();

        if name.is_empty() || base_url.is_empty() || model_id.is_empty() {
            return;
        }

        if let Some(ref edit_id) = self.ai_edit_model_id.clone() {
            // Update existing model
            let model = AiModelConfig {
                id: edit_id.clone(),
                name,
                base_url,
                api_key,
                model_id,
                enabled: true,
                description: desc,
            };
            settings_entity(cx).update(cx, |state, cx| {
                state.update_ai_model(model, cx);
            });
        } else {
            // Add new model
            let mut model = AiModelConfig::new(name, base_url, api_key, model_id);
            model.description = desc;
            settings_entity(cx).update(cx, |state, cx| {
                state.add_ai_model(model, cx);
            });
        }

        self.sync_ai_default_model_select(cx);
        self.close_add_model_dialog(window, cx);
    }

    /// Rebuild the default-model select options from the current AI models so
    /// the dropdown reflects freshly added/edited/removed models immediately,
    /// without reopening the settings panel. Preserves the currently selected
    /// value when it still exists.
    fn sync_ai_default_model_select(&mut self, cx: &mut Context<Self>) {
        let models = settings_entity(cx).read(cx).settings.ai_models.clone();
        let selected = self.ai_default_model_select.read(cx).selected_value().cloned();
        let options = models
            .iter()
            .map(|m| {
                SelectOption::new(m.id.clone(), format!("{} ({})", m.name, m.model_id))
            })
            .collect();
        self.ai_default_model_select.update(cx, |s, cx| {
            s.set_options(options, cx);
            s.set_selected_value(selected, cx);
        });
    }

    fn test_model_from_dialog(&mut self, cx: &mut Context<Self>) {
        let base_url = self.ai_model_base_url_input.read(cx).text().to_string();
        let api_key = self.ai_model_api_key_input.read(cx).text().to_string();
        let model_id = self.ai_model_id_input.read(cx).text().to_string();

        if base_url.trim().is_empty() || model_id.trim().is_empty() {
            log::warn!(
                "[AI Model Test] Validation failed: base_url or model_id is empty (base_url: '{}', model_id: '{}')",
                base_url,
                model_id
            );
            self.ai_test_result = Some(Err(i18n!(cx, "status.test_failed")));
            cx.notify();
            return;
        }

        self.ai_test_in_progress = true;
        self.ai_test_result = None;
        cx.notify();

        let log_base_url = base_url.clone();
        let log_model_id = model_id.clone();

        cx.spawn(async move |this, cx| {
            let test_result = smol::unblock(move || {
                velowork_ai::provider::test_llm_connection(&base_url, &api_key, &model_id, 15)
            }).await;

            match &test_result {
                Ok(()) => {
                    log::info!(
                        "[AI Model Test] Successfully connected to model '{}' via '{}'",
                        log_model_id,
                        log_base_url
                    );
                }
                Err(err) => {
                    log::error!(
                        "[AI Model Test] Failed to connect to model '{}' via '{}': {}",
                        log_model_id,
                        log_base_url,
                        err
                    );
                }
            }

            this.update(cx, |this, cx| {
                this.ai_test_in_progress = false;
                this.ai_test_result = Some(
                    test_result
                        .map(|_| i18n!(cx, "status.test_success"))
                        .map_err(|e| e),
                );
                cx.notify();
            }).ok();
        }).detach();
    }
}
