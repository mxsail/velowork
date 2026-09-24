use crate::keybindings::Cancel;
use crate::theme::{theme, with_alpha};
use crate::ui::tokens::{
    ui_text_md, ui_text_ms, ui_text_sm, RADIUS_MD, RADIUS_STD, SPACE_LG, SPACE_MD, SPACE_SM,
    SPACE_XL, SPACE_XS,
};
use crate::views::components::{modal_content, modal_header};
use gpui::prelude::*;
use gpui::*;
use velowork_i18n::i18n;
use velowork_ui::button::Button;
use velowork_ui::design::appearance::ControlSize;
use velowork_ui::focus_group::{FocusGroup, FocusGroupExt};
use velowork_ui::simple_input::SimpleInput;
use velowork_ui::{h_flex, v_flex};

use super::{PendingFocus, ProfileManager};

impl ProfileManager {
    pub(super) fn render_profile_row(
        &self,
        ix: usize,
        entry: &velowork_core::profiles::ProfileEntry,
        is_list_focused: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let t = theme(cx);
        let id = entry.id.clone();
        let display_name = entry.display_name.clone();
        let is_active = id == self.active_id;
        let is_deleting = self.show_delete_confirmation.as_deref() == Some(&id);
        let is_default = id == self.default_profile_id;
        let is_selected = ix == self.selected_index;

        let id_for_switch = id.clone();
        let id_for_dir = id.clone();
        let id_for_delete = id.clone();
        let id_for_delete_confirm = id.clone();
        let entity = cx.entity();

        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);
        let border_color = if is_deleting {
            with_alpha(t.error, 0.4)
        } else if is_selected && is_list_focused {
            p.border_active
        } else if is_selected {
            p.surface_accent.opacity(0.6)
        } else if is_active {
            p.surface_accent
        } else {
            p.border_subtle
        };

        let bg_color = if is_deleting {
            with_alpha(t.error, 0.08)
        } else if is_selected && is_list_focused {
            p.surface_hover
        } else if is_active {
            p.surface_accent.opacity(0.15)
        } else {
            p.surface_card
        };

        let mut row = h_flex()
            .justify_between()
            .items_center()
            .px(SPACE_LG)
            .py(SPACE_MD)
            .rounded(RADIUS_MD)
            .border_1()
            .border_color(border_color)
            .bg(bg_color)
            .hover(|s| s.bg(p.surface_hover));

        if is_selected && is_list_focused && !is_deleting {
            let ring_color = Hsla {
                a: 0.35,
                ..p.border_active
            };
            row = row.shadow(vec![BoxShadow {
                color: ring_color,
                offset: point(px(0.0), px(0.0)),
                blur_radius: px(3.0),
                spread_radius: px(1.5),
                inset: false,
            }]);
        }

        row = row.on_mouse_down(MouseButton::Left, {
            let e = entity.clone();
            move |_, _window, cx| {
                e.update(cx, |this, cx| {
                    if this.show_delete_confirmation.is_some() {
                        this.show_delete_confirmation = None;
                    }
                    this.selected_index = ix;
                    this.pending_focus = Some(PendingFocus::List);
                    cx.notify();
                });
            }
        });

        if is_deleting {
            row.child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .w_full()
                    .on_key_down(cx.listener({
                        let id_del = id_for_delete_confirm.clone();
                        move |this, event: &KeyDownEvent, window, cx| match event.keystroke.key.as_str() {
                            "left" | "right" => {
                                cx.stop_propagation();
                                if this.delete_cancel_focus.is_focused(window) {
                                    this.pending_focus = Some(PendingFocus::DeleteConfirm);
                                } else {
                                    this.pending_focus = Some(PendingFocus::DeleteCancel);
                                }
                                cx.notify();
                            }
                            "escape" => {
                                cx.stop_propagation();
                                this.cancel_delete(cx);
                            }
                            "delete" if this.delete_confirm_focus.is_focused(window) => {
                                cx.stop_propagation();
                                this.delete_profile(&id_del, cx);
                            }
                            _ => {}
                        }
                    }))
                    .child(
                        v_flex()
                            .gap(SPACE_XS)
                            .child(
                                div()
                                    .text_size(ui_text_md(cx))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(t.error))
                                    .child(
                                        i18n!(cx, "profile.manager.delete_confirm")
                                            .replace("{name}", &display_name),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(ui_text_sm(cx))
                                    .text_color(rgb(t.text_muted))
                                    .child(i18n!(cx, "profile.manager.delete_hint")),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap(SPACE_SM)
                            .items_center()
                            .child(
                                Button::new(format!("profile-del-cancel-{id}"), &t)
                                    .focus_handle(&self.delete_cancel_focus)
                                    .size(ControlSize::Default)
                                    .label(i18n!(cx, "common.action.cancel"))
                                    .on_click({
                                        let e = entity.clone();
                                        move |_, _window, cx| {
                                            e.update(cx, |this, cx| this.cancel_delete(cx))
                                        }
                                    }),
                            )
                            .child(
                                Button::new(format!("profile-del-confirm-{id}"), &t)
                                    .focus_handle(&self.delete_confirm_focus)
                                    .size(ControlSize::Default)
                                    .danger(true)
                                    .label(i18n!(cx, "common.action.delete"))
                                    .on_click({
                                        let e = entity.clone();
                                        let id_del = id_for_delete_confirm.clone();
                                        move |_, _window, cx| {
                                            e.update(cx, |this, cx| {
                                                this.delete_profile(&id_del, cx)
                                            })
                                        }
                                    }),
                            ),
                    ),
            )
        } else {
            row.child(
                h_flex()
                    .gap(SPACE_SM)
                    .items_center()
                    .child(
                        div()
                            .text_size(ui_text_md(cx))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(t.text_primary))
                            .child(display_name),
                    )
                    .when(is_active, |d| {
                        d.child(
                            div()
                                .px(SPACE_SM)
                                .py(px(1.0))
                                .rounded(RADIUS_STD)
                                .bg(with_alpha(t.accent, 0.15))
                                .text_size(ui_text_ms(cx))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(rgb(t.accent))
                                .child(i18n!(cx, "profile.manager.active")),
                        )
                    }),
            )
            .child(
                h_flex()
                    .gap(SPACE_SM)
                    .items_center()
                    .child(
                        Button::new(format!("profile-switch-{id}"), &t)
                            .size(ControlSize::Default)
                            .primary()
                            .disabled(is_active)
                            .label(i18n!(cx, "common.action.switch"))
                            .on_click({
                                let e = entity.clone();
                                move |_, _window, cx| {
                                    e.update(cx, |this, cx| {
                                        this.switch_to(id_for_switch.clone(), cx)
                                    })
                                }
                            }),
                    )
                    .child(
                        Button::new(format!("profile-dir-{id}"), &t)
                            .size(ControlSize::Default)
                            .label(i18n!(cx, "profile.manager.open_dir"))
                            .on_click({
                                let e = entity.clone();
                                move |_, _window, cx| {
                                    e.update(cx, |this, _cx| {
                                        this.open_profile_dir(&id_for_dir);
                                    })
                                }
                            }),
                    )
                    .child(
                        Button::new(format!("profile-delete-{id}"), &t)
                            .size(ControlSize::Default)
                            .danger(true)
                            .disabled(is_active || is_default)
                            .label(i18n!(cx, "common.action.delete"))
                            .on_click({
                                let e = entity.clone();
                                let id_del = id_for_delete.clone();
                                move |_, _window, cx| {
                                    e.update(cx, |this, cx| {
                                        this.selected_index = ix;
                                        this.confirm_delete(&id_del, cx);
                                    })
                                }
                            }),
                    ),
            )
        }
    }
}

impl Render for ProfileManager {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);
        let focus_handle = self.focus_handle.clone();
        let error_message = self.error_message.clone();
        let profiles = self.profiles.clone();
        let new_profile_input = self.new_profile_input.clone();

        // Process any pending focus state change safely
        if let Some(target) = self.pending_focus.take() {
            match target {
                PendingFocus::List => {
                    window.focus(&self.list_focus, cx);
                    if !self.profiles.is_empty() {
                        self.scroll_handle.scroll_to_item(self.selected_index);
                    }
                }
                PendingFocus::DeleteCancel => {
                    window.focus(&self.delete_cancel_focus, cx);
                }
                PendingFocus::DeleteConfirm => {
                    window.focus(&self.delete_confirm_focus, cx);
                }
            }
        }

        // Build focus group for Tab / Shift+Tab cycling
        let focus_group = FocusGroup::new();
        if self.show_delete_confirmation.is_some() {
            focus_group.add(self.delete_cancel_focus.clone());
            focus_group.add(self.delete_confirm_focus.clone());
        } else {
            focus_group.add(self.list_focus.clone());
            focus_group.add(self.new_profile_input.read(cx).focus_handle(cx));
            focus_group.add(self.create_button_focus.clone());
        }

        let is_list_focused = self.list_focus.is_focused(window);

        modal_content("profile-manager-modal", cx)
            .w(px(560.0))
            .max_h(px(600.0))
            .track_focus(&focus_handle)
            .tab_cycle(&focus_group)
            .key_context("ProfileManager")
            .on_action(cx.listener(|this, _: &Cancel, _window, cx| {
                if this.show_delete_confirmation.is_some() {
                    this.cancel_delete(cx);
                } else {
                    this.close(cx);
                }
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            })
                    .child(modal_header(
                        i18n!(cx, "profile.manager.title"),
                        Some(&i18n!(cx, "profile.manager.subtitle")),
                        &t,
                        cx,
                        cx.listener(|this, _, _window, cx| this.close(cx)),
                    ))
                    .when_some(error_message, |d, msg| {
                        d.child(
                            div()
                                .px(SPACE_XL)
                                .py(SPACE_MD)
                                .bg(with_alpha(t.error, 0.1))
                                .border_b_1()
                                .border_color(p.border_subtle)
                                .child(
                                    div()
                                        .text_size(ui_text_md(cx))
                                        .text_color(rgb(t.error))
                                        .child(msg),
                                   ),
                        )
                    })
                    .child(
                        v_flex()
                            .px(SPACE_XS)
                            .py(SPACE_MD)
                            .gap(SPACE_MD)
                            .flex_1()
                            .min_h_0()
                            .child(
                                // Profile card list
                                v_flex()
                                    .id("profile-list")
                                    .track_focus(&self.list_focus)
                                    .track_scroll(&self.scroll_handle)
                                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                                        this.handle_list_key_down(event, cx);
                                    }))
                                    .gap(SPACE_SM)
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_y_scroll()
                                    .when(profiles.is_empty(), |d| {
                                        d.flex()
                                            .items_center()
                                            .justify_center()
                                            .p(px(32.0))
                                            .child(
                                                div()
                                                    .text_size(ui_text_md(cx))
                                                    .text_color(rgb(t.text_muted))
                                                    .child(i18n!(cx, "profile.manager.no_profiles")),
                                            )
                                    })
                                    .when(!profiles.is_empty(), |d| {
                                        d.children(
                                            profiles
                                                .iter()
                                                .enumerate()
                                                .map(|(ix, p)| self.render_profile_row(ix, p, is_list_focused, cx)),
                                        )
                                    }),
                            ),
                    )
                    .child(
                        // Create new profile footer (aligned width and height)
                        h_flex()
                            .h(px(48.0))
                            .flex_shrink_0()
                            .w_full()
                            .gap(SPACE_MD)
                            .px(SPACE_XS)
                            .border_t_1()
                            .border_color(p.border_subtle)
                            .items_center()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                                        if event.keystroke.key.as_str() == "enter" {
                                            cx.stop_propagation();
                                            this.create_profile(cx);
                                        }
                                    }))
                                    .child(SimpleInput::new(&new_profile_input)),
                            )
                            .child(
                                Button::new("create-profile-btn", &t)
                                    .focus_handle(&self.create_button_focus)
                                    .size(ControlSize::Default)
                                    .primary()
                                    .label(i18n!(cx, "common.action.create"))
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.create_profile(cx);
                                    })),
                            ),
                    )
    }
}

