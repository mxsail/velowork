use crate::keybindings::Cancel;
use crate::settings::settings_entity;
use crate::theme::theme;
use crate::ui::tokens::{
    ui_text_md, ui_text_sm, SPACE_XS, SPACE_SM, SPACE_MD, SPACE_LG, SPACE_XL,
    RADIUS_LG, dialog_md,
};
use crate::workspace::settings::{SearchEngineConfig, SearchEngineConfig as Engine};
use gpui::*;
use gpui::prelude::*;
use velowork_ui::v_flex;
use velowork_ui::h_flex;
use velowork_ui::button;
use velowork_ui::button_primary;
use velowork_ui::ControlSize;
use velowork_ui::dialog_actions::dialog_actions;
use velowork_ui::focus_group::{FocusGroup, FocusGroupExt};
use crate::views::components::{modal_backdrop, modal_content, modal_header};
use velowork_i18n::i18n;

use super::components::*;
use super::SettingsPanel;

impl SettingsPanel {
    /// Render the "Search Engines" settings page.
    pub(super) fn render_search(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let engines = {
            let guard = settings_entity(cx).read(cx);
            guard.settings.search_engines.clone()
        };

        let desc = i18n!(cx, "settings.search_engines.desc");
        let add_label = i18n!(cx, "settings.search_engines.add");
        let empty_label = i18n!(cx, "settings.search_engines.empty");

        v_flex()
            .gap(SPACE_LG)
            .child(section_header(&i18n!(cx, "settings.nav.search_engines"), &t, cx))
            .child(
                div()
                    .text_size(ui_text_sm(cx))
                    .text_color(rgb(t.text_muted))
                    .child(desc),
            )
            .child(if engines.is_empty() {
                div()
                    .text_size(ui_text_sm(cx))
                    .text_color(rgb(t.text_muted))
                    .child(empty_label)
                    .into_any_element()
            } else {
                self.render_search_list(&engines, &t, cx).into_any_element()
            })
            .child(
                // Add button below the list (consistent with "Add AI Model")
                div()
                    .mx(SPACE_LG)
                    .my(SPACE_MD)
                    .child({
                        let add_fh = self.get_or_create_button_focus_handle("search-add-engine-btn", cx);
                        button_primary("search-add-engine-btn", add_label.clone(), &t)
                            .full_width(true)
                            .focus_handle(&add_fh)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_add_search_dialog(window, cx);
                            }))
                    }),
            )
    }

    /// Render the list of configured search engines (cards).
    fn render_search_list(
        &self,
        engines: &[SearchEngineConfig],
        t: &velowork_core::theme::ThemeColors,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let edit_label = i18n!(cx, "common.edit");
        let delete_label = i18n!(cx, "common.delete");
        let keyword_label = i18n!(cx, "settings.search_engines.keyword");
        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);

        v_flex()
            .mx(SPACE_LG)
            .gap_0()
            .rounded(RADIUS_LG)
            .border_1()
            .border_color(p.border_subtle)
            .bg(p.surface_card)
            .overflow_hidden()
            .children(engines.iter().enumerate().map(|(idx, engine)| {
                let id = engine.id.clone();
                let eid = format!("search-engine-{}", id);
                let name = engine.name.clone();
                let keyword = engine.keyword.clone();
                let url = engine.url.clone();
                let enabled = engine.enabled;
                let is_last = idx == engines.len() - 1;

                div()
                    .id(ElementId::Name(eid.into()))
                    .px(SPACE_LG)
                    .py(SPACE_MD)
                    .when(!is_last, |d| {
                        d.border_b_1().border_color(p.border_subtle)
                    })
                    .flex()
                    .items_center()
                    .gap(SPACE_LG)
                    // Engine name + keyword + url
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
                                    .child(name),
                            )
                            .child(
                                h_flex()
                                    .gap(SPACE_MD)
                                    .child(
                                        div()
                                            .text_size(ui_text_sm(cx))
                                            .text_color(rgb(t.accent))
                                            .child(format!("{}: {}", keyword_label, keyword)),
                                    )
                                    .child(
                                        div()
                                            .text_size(ui_text_sm(cx))
                                            .text_color(rgb(t.text_muted))
                                            .font_family("monospace")
                                            .truncate()
                                            .child(url),
                                    ),
                            ),
                    )
                    // Toggle + Edit + Delete
                    .child({
                        let toggle_fh = self.get_or_create_toggle_focus_handle(&format!("search-engine-toggle-{}", id), cx);
                        let edit_fh = self.get_or_create_button_focus_handle(&format!("search-engine-edit-{}", id), cx);
                        let delete_fh = self.get_or_create_button_focus_handle(&format!("search-engine-delete-{}", id), cx);
                        h_flex()
                            .gap(SPACE_SM)
                            .items_center()
                            // Enabled toggle
                            .child({
                                let toggle_id = id.clone();
                                velowork_ui::Switch::new(format!("search-engine-toggle-{}", id))
                                    .focus(&toggle_fh)
                                    .checked(enabled)
                                    .on_click(cx.listener(move |_, checked: &bool, _window, cx| {
                                        let new_state = *checked;
                                        let tid = toggle_id.clone();
                                        settings_entity(cx).update(cx, |state, cx| {
                                            state.toggle_search_engine(&tid, new_state, cx);
                                        });
                                    }))
                            })
                            // Edit button
                            .child({
                                let edit_id = id.clone();
                                button(
                                    ElementId::Name(format!("search-engine-edit-{}", id).into()),
                                    edit_label.clone(),
                                    t,
                                )
                                .size(ControlSize::Compact)
                                .focus_handle(&edit_fh)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.open_edit_search_dialog(&edit_id, window, cx);
                                }))
                            })
                            // Delete button
                            .child({
                                let del_id = id.clone();
                                button(
                                    ElementId::Name(format!("search-engine-delete-{}", id).into()),
                                    delete_label.clone(),
                                    t,
                                )
                                .size(ControlSize::Compact)
                                .danger(true)
                                .focus_handle(&delete_fh)
                                .on_click(cx.listener(move |_, _, _, cx| {
                                        settings_entity(cx).update(cx, |state, cx| {
                                            state.remove_search_engine(&del_id, cx);
                                        });
                                    }))
                            })
                    })
            }))
    }

    /// Render the add/edit search engine dialog.
    pub(super) fn render_search_dialog(
        &self,
        t: &velowork_core::theme::ThemeColors,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let title = if self.search_edit_id.is_some() {
            i18n!(cx, "settings.search_engines.edit_title")
        } else {
            i18n!(cx, "settings.search_engines.add_title")
        };
        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);
        let name_label = i18n!(cx, "settings.search_engines.name");
        let url_label = i18n!(cx, "settings.search_engines.url");
        let url_hint = i18n!(cx, "settings.search_engines.url_hint");
        let keyword_label = i18n!(cx, "settings.search_engines.keyword");
        let keyword_hint = i18n!(cx, "settings.search_engines.keyword_hint");
        let cancel_label = i18n!(cx, "common.cancel");
        let save_label = i18n!(cx, "common.save");
        let focus_group = FocusGroup::new();
        focus_group.extend([
            self.search_name_input.read(cx).focus_handle(cx),
            self.search_url_input.read(cx).focus_handle(cx),
            self.search_keyword_input.read(cx).focus_handle(cx),
            self.search_dialog_cancel_focus.clone(),
            self.search_dialog_save_focus.clone(),
        ]);

        // Backdrop
        let weak = cx.weak_entity();
        let weak_cancel = weak.clone();
        let weak_save = weak.clone();
        modal_backdrop("search-engine-dialog-backdrop", &t, cx)
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| {
                this.close_search_dialog(Some(window), cx);
            }))
            .tab_cycle(&focus_group)
            .child(
                modal_content("search-engine-dialog", cx)
                    .w(dialog_md(cx))
                    .on_action(cx.listener(|this, _: &Cancel, window, cx| {
                        this.close_search_dialog(Some(window), cx);
                    }))
                    .child(modal_header(
                        title,
                        Option::<String>::None,
                        &t,
                        cx,
                        cx.listener(|this, _, window, cx| this.close_search_dialog(Some(window), cx)),
                    ))
                    .child(
                        v_flex()
                            .p(SPACE_LG)
                            .gap(SPACE_XL)
                            // Name
                            .child(self.render_dialog_input(
                                "search-dialog-name",
                                &name_label,
                                &self.search_name_input,
                                t,
                                cx,
                            ))
                            // URL
                            .child(
                                v_flex()
                                    .gap(SPACE_XS)
                                    .child(self.render_dialog_input(
                                        "search-dialog-url",
                                        &url_label,
                                        &self.search_url_input,
                                        t,
                                        cx,
                                    ))
                                    .child(
                                        div()
                                            .text_size(ui_text_sm(cx))
                                            .text_color(rgb(t.text_muted))
                                            .child(url_hint),
                                    ),
                            )
                            // Keyword
                            .child(
                                v_flex()
                                    .gap(SPACE_XS)
                                    .child(self.render_dialog_input(
                                        "search-dialog-keyword",
                                        &keyword_label,
                                        &self.search_keyword_input,
                                        t,
                                        cx,
                                    ))
                                    .child(
                                        div()
                                            .text_size(ui_text_sm(cx))
                                            .text_color(rgb(t.text_muted))
                                            .child(keyword_hint),
                                    ),
                            ),
                    )
                    .child(
                        h_flex()
                            .h(px(52.0))
                            .flex_shrink_0()
                            .px(SPACE_LG)
                            .border_t_1()
                            .border_color(p.border_subtle)
                            .items_center()
                            .justify_end()
                            .child(
                                dialog_actions(
                                    cancel_label,
                                    move |_, window, cx| {
                                        weak_cancel.update(cx, |this, cx| this.close_search_dialog(Some(window), cx))
                                            .ok();
                                    },
                                    save_label,
                                    move |_, window, cx| {
                                        weak_save.update(cx, |this, cx| this.save_search_from_dialog(Some(window), cx))
                                            .ok();
                                    },
                                    &self.search_dialog_cancel_focus,
                                    &self.search_dialog_save_focus,
                                    &t,
                                ),
                            ),
                    ),
            )
    }

    fn open_add_search_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dialog_trigger_focus_handle = Some(self.get_or_create_button_focus_handle("search-add-engine-btn", cx));
        self.search_add_dialog_open = true;
        self.search_edit_id = None;
        self.search_name_input
            .update(cx, |s, cx| s.set_value("", cx));
        self.search_url_input
            .update(cx, |s, cx| s.set_value("", cx));
        self.search_keyword_input
            .update(cx, |s, cx| s.set_value("", cx));
        self.search_name_input.update(cx, |s, cx| s.focus(window, cx));
        cx.notify();
    }

    fn open_edit_search_dialog(&mut self, engine_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let s = settings_entity(cx).read(cx).settings.clone();
        if let Some(engine) = s.search_engines.iter().find(|e| e.id == engine_id) {
            self.dialog_trigger_focus_handle = Some(self.get_or_create_button_focus_handle(&format!("search-engine-edit-{}", engine_id), cx));
            self.search_add_dialog_open = true;
            self.search_edit_id = Some(engine_id.to_string());
            self.search_name_input
                .update(cx, |s, cx| s.set_value(engine.name.clone(), cx));
            self.search_url_input
                .update(cx, |s, cx| s.set_value(engine.url.clone(), cx));
            self.search_keyword_input
                .update(cx, |s, cx| s.set_value(engine.keyword.clone(), cx));
            self.search_name_input.update(cx, |s, cx| s.focus(window, cx));
            cx.notify();
        }
    }

    pub(super) fn close_search_dialog(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        self.search_add_dialog_open = false;
        self.search_edit_id = None;
        if let Some(window) = window
            && let Some(fh) = self.dialog_trigger_focus_handle.take()
        {
            window.focus(&fh, cx);
        }
        cx.notify();
    }

    fn save_search_from_dialog(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        let name = self.search_name_input.read(cx).text().to_string();
        let url = self.search_url_input.read(cx).text().to_string();
        let keyword = self.search_keyword_input.read(cx).text().to_string();

        if name.is_empty() || url.is_empty() || !url.contains("%s") {
            return;
        }

        if let Some(ref edit_id) = self.search_edit_id.clone() {
            let engine = Engine {
                id: edit_id.clone(),
                name,
                url,
                keyword,
                enabled: true,
            };
            settings_entity(cx).update(cx, |state, cx| {
                state.update_search_engine(engine, cx);
            });
        } else {
            let engine = SearchEngineConfig::new(name, url, keyword);
            settings_entity(cx).update(cx, |state, cx| {
                state.add_search_engine(engine, cx);
            });
        }

        self.close_search_dialog(window, cx);
    }
}
