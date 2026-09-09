use crate::settings::settings_entity;
use crate::theme::theme;
use crate::ui::tokens::ui_text_md;
use crate::views::overlays::settings::settings_panel::components::*;
use gpui::*;
use gpui::prelude::*;
use velowork_i18n::i18n;
use velowork_ui::button::{button, Button};
use velowork_ui::input::{Input, InputState};
use velowork_ui::tokens::{SPACE_XS, SPACE_SM, SPACE_MD, SPACE_LG};
use velowork_workspace::security::current_security_service;

use super::SettingsPanel;

impl SettingsPanel {
    pub(super) fn render_security(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let s = settings_entity(cx).read(cx).settings.clone();

        let security_label = i18n!(cx, "settings.nav.security");
        let current_mode_label = i18n!(cx, "settings.security.current_mode");
        let mode_standard = i18n!(cx, "settings.security.mode.standard");
        let mode_enhanced = i18n!(cx, "settings.security.mode.enhanced");
        let enable_enhanced_label = i18n!(cx, "settings.security.enable_enhanced");
        let change_pw_label = i18n!(cx, "settings.security.change_master_password");
        let remove_pw_label = i18n!(cx, "settings.security.remove_master_password");
        let current_pw_placeholder = i18n!(cx, "settings.security.current_password_placeholder");
        let new_pw_placeholder = i18n!(cx, "settings.security.new_password_placeholder");
        let master_pw_placeholder = i18n!(cx, "settings.security.master_password_placeholder");
        let confirm_pw_placeholder = i18n!(cx, "settings.security.confirm_password_placeholder");
        let password_timeout_label = i18n!(cx, "settings.security.password_timeout.label");
        let remove_hint_label = i18n!(cx, "settings.security.remove_hint");
        let confirm_label = i18n!(cx, "common.confirm");
        let cancel_label = i18n!(cx, "common.cancel");

        let is_enhanced = s.security.security_mode == "enhanced";
        let result = self.security_result.clone();

        div()
            .child(section_header(&security_label, &t, cx))
            .child(
                section_container(&t)
                    // 当前模式
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
                                    .child(current_mode_label),
                            )
                            .child(
                                div()
                                    .text_size(ui_text_md(cx))
                                    .text_color(rgb(t.text_primary))
                                    .child(if is_enhanced {
                                        mode_enhanced.clone()
                                    } else {
                                        mode_standard.clone()
                                    }),
                            ),
                    )
                    // 标准模式：默认仅显示「启用增强模式」按钮；点击后展开密码/确认密码输入
                    .when(!is_enhanced, |d| {
                        d.child(
                            div()
                                .px(SPACE_LG)
                                .py(SPACE_MD)
                                .flex()
                                .flex_col()
                                .gap(SPACE_MD)
                                // 默认：仅显示「启用增强模式」按钮（适中宽度，不占满整行）
                                .when(!self.security_setup_mode, |d| {
                                    let enable_fh = self.get_or_create_button_focus_handle("security-enable-enhanced", cx);
                                    d.child(
                                        div()
                                            .flex()
                                            .child(
                                                button(
                                                    "security-enable-enhanced",
                                                    enable_enhanced_label.clone(),
                                                    &t,
                                                )
                                                .focus_handle(&enable_fh)
                                                .on_click(cx.listener(
                                                    |this, _, window, cx| {
                                                        this.dialog_trigger_focus_handle = Some(this.get_or_create_button_focus_handle("security-enable-enhanced", cx));
                                                        this.security_setup_mode = true;
                                                        this.security_new_password_input.update(cx, |s, cx| s.focus(window, cx));
                                                        cx.notify();
                                                    },
                                                )),
                                            ),
                                    )
                                })
                                // 展开：密码 + 确认密码 + 确认/取消
                                .when(self.security_setup_mode, |d| {
                                    let confirm_fh = self.get_or_create_button_focus_handle("security-setup-confirm", cx);
                                    let cancel_fh = self.get_or_create_button_focus_handle("security-setup-cancel", cx);
                                    d.child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(SPACE_SM)
                                            .child(
                                                div()
                                                    .text_size(ui_text_md(cx))
                                                    .text_color(rgb(t.text_muted))
                                                    .child(master_pw_placeholder.clone()),
                                            )
                                            .child(self.render_password_field(&self.security_new_password_input))
                                            .child(
                                                div()
                                                    .text_size(ui_text_md(cx))
                                                    .text_color(rgb(t.text_muted))
                                                    .child(confirm_pw_placeholder.clone()),
                                             )
                                            .child(self.render_password_field(&self.security_confirm_password_input))
                                            .child(
                                                div()
                                                    .flex()
                                                    .gap(SPACE_MD)
                                                    .child(
                                                        Button::new("security-setup-confirm", &t)
                                                            .label(confirm_label.clone())
                                                            .primary()
                                                            .loading(self.security_busy)
                                                            .focus_handle(&confirm_fh)
                                                            .on_click(cx.listener(
                                                                |this, _, window, cx| {
                                                                    this.security_busy = true;
                                                                    this
                                                                        .security_setup_master_password(
                                                                            window,
                                                                            cx,
                                                                        );
                                                                },
                                                            )),
                                                    )
                                                    .child(
                                                        button(
                                                            "security-setup-cancel",
                                                            cancel_label.clone(),
                                                            &t,
                                                        )
                                                        .focus_handle(&cancel_fh)
                                                        .on_click(cx.listener(
                                                            |this, _, window, cx| {
                                                                this.security_setup_mode =
                                                                    false;
                                                                this
                                                                    .security_new_password_input
                                                                    .update(cx, |s, cx| {
                                                                        s.set_value("", cx);
                                                                    });
                                                                this
                                                                    .security_confirm_password_input
                                                                    .update(cx, |s, cx| {
                                                                        s.set_value("", cx);
                                                                    });
                                                                if let Some(fh) = this.dialog_trigger_focus_handle.take() {
                                                                    window.focus(&fh, cx);
                                                                } else {
                                                                    let fh = this.get_or_create_button_focus_handle("security-enable-enhanced", cx);
                                                                    window.focus(&fh, cx);
                                                                }
                                                                cx.notify();
                                                            },
                                                        )),
                                                    ),
                                            ),
                                    )
                                }),
                        )
                    })
                    // 增强模式：修改 / 移除主密码（默认仅显示两个入口按钮）
                    .when(is_enhanced, |d| {
                        d.child(
                            div()
                                .px(SPACE_LG)
                                .py(SPACE_MD)
                                .flex()
                                .flex_col()
                                .gap(SPACE_SM)
                                .when(
                                    !self.security_change_mode && !self.security_remove_mode,
                                    |d| {
                                        let change_fh = self.get_or_create_button_focus_handle("security-change-pw", cx);
                                        let remove_fh = self.get_or_create_button_focus_handle("security-remove-pw", cx);
                                        d.child(
                                            div()
                                                .flex()
                                                .gap(SPACE_MD)
                                                .child(
                                                    button(
                                                        "security-change-pw",
                                                        change_pw_label.clone(),
                                                        &t,
                                                    )
                                                    .focus_handle(&change_fh)
                                                    .on_click(cx.listener(
                                                        |this, _, window, cx| {
                                                            this.dialog_trigger_focus_handle = Some(this.get_or_create_button_focus_handle("security-change-pw", cx));
                                                            this.security_change_mode = true;
                                                            this.security_current_password_input.update(cx, |s, cx| s.focus(window, cx));
                                                            cx.notify();
                                                        },
                                                    )),
                                                )
                                                .child(
                                                    button(
                                                        "security-remove-pw",
                                                        remove_pw_label.clone(),
                                                        &t,
                                                    )
                                                    .focus_handle(&remove_fh)
                                                    .on_click(cx.listener(
                                                        |this, _, window, cx| {
                                                            this.dialog_trigger_focus_handle = Some(this.get_or_create_button_focus_handle("security-remove-pw", cx));
                                                            this.security_remove_mode = true;
                                                            this.security_current_password_input.update(cx, |s, cx| s.focus(window, cx));
                                                            cx.notify();
                                                        },
                                                    )),
                                                ),
                                        )
                                    },
                                )
                                // 修改主密码：点击「修改主密码」后才展开当前/新密码输入
                                .when(self.security_change_mode, |d| {
                                    let change_confirm_fh = self.get_or_create_button_focus_handle("security-change-confirm", cx);
                                    let change_cancel_fh = self.get_or_create_button_focus_handle("security-change-cancel", cx);
                                    d.child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(SPACE_SM)
                                            .child(
                                                div()
                                                    .text_size(ui_text_md(cx))
                                                    .text_color(rgb(t.text_muted))
                                                    .child(current_pw_placeholder.clone()),
                                            )
                                            .child(self.render_password_field(&self.security_current_password_input))
                                            .child(
                                                div()
                                                    .text_size(ui_text_md(cx))
                                                    .text_color(rgb(t.text_muted))
                                                    .child(new_pw_placeholder.clone()),
                                            )
                                            .child(self.render_password_field(&self.security_new_password_input))
                                            .child(
                                                div()
                                                    .flex()
                                                    .gap(SPACE_MD)
                                                    .child(
                                                        Button::new("security-change-confirm", &t)
                                                            .label(confirm_label.clone())
                                                            .primary()
                                                            .loading(self.security_busy)
                                                            .focus_handle(&change_confirm_fh)
                                                            .on_click(cx.listener(
                                                                |this, _, window, cx| {
                                                                    this.security_busy = true;
                                                                    this
                                                                        .security_change_master_password(
                                                                            window,
                                                                            cx,
                                                                        );
                                                                },
                                                            )),
                                                    )
                                                    .child(
                                                        button(
                                                            "security-change-cancel",
                                                            cancel_label.clone(),
                                                            &t,
                                                        )
                                                        .focus_handle(&change_cancel_fh)
                                                        .on_click(cx.listener(
                                                            |this, _, window, cx| {
                                                                this.security_change_mode = false;
                                                                this
                                                                    .security_current_password_input
                                                                    .update(cx, |st, cx| {
                                                                        st.set_value("", cx);
                                                                    });
                                                                this
                                                                    .security_new_password_input
                                                                    .update(cx, |st, cx| {
                                                                        st.set_value("", cx);
                                                                    });
                                                                if let Some(fh) = this.dialog_trigger_focus_handle.take() {
                                                                    window.focus(&fh, cx);
                                                                } else {
                                                                    let fh = this.get_or_create_button_focus_handle("security-change-pw", cx);
                                                                    window.focus(&fh, cx);
                                                                }
                                                                cx.notify();
                                                            },
                                                        )),
                                                    ),
                                            ),
                                    )
                                })
                                // 移除主密码：点击「移除主密码」后才展开当前密码确认输入
                                .when(self.security_remove_mode, |d| {
                                    let remove_confirm_fh = self.get_or_create_button_focus_handle("security-remove-confirm", cx);
                                    let remove_cancel_fh = self.get_or_create_button_focus_handle("security-remove-cancel", cx);
                                    d.child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(SPACE_SM)
                                            .child(
                                                div()
                                                    .text_size(ui_text_md(cx))
                                                    .text_color(rgb(t.text_muted))
                                                    .child(remove_hint_label.clone()),
                                            )
                                            .child(self.render_password_field(&self.security_current_password_input))
                                            .child(
                                                div()
                                                    .flex()
                                                    .gap(SPACE_MD)
                                                    .child(
                                                        Button::new("security-remove-confirm", &t)
                                                            .label(confirm_label.clone())
                                                            .danger(true)
                                                            .loading(self.security_busy)
                                                            .focus_handle(&remove_confirm_fh)
                                                            .on_click(cx.listener(
                                                                |this, _, window, cx| {
                                                                    this.security_busy = true;
                                                                    this
                                                                        .security_remove_master_password(
                                                                            window,
                                                                            cx,
                                                                        );
                                                                },
                                                            )),
                                                    )
                                                    .child(
                                                        button(
                                                            "security-remove-cancel",
                                                            cancel_label.clone(),
                                                            &t,
                                                        )
                                                        .focus_handle(&remove_cancel_fh)
                                                        .on_click(cx.listener(
                                                            |this, _, window, cx| {
                                                                this.security_remove_mode = false;
                                                                this
                                                                    .security_current_password_input
                                                                    .update(cx, |st, cx| {
                                                                        st.set_value("", cx);
                                                                    });
                                                                if let Some(fh) = this.dialog_trigger_focus_handle.take() {
                                                                    window.focus(&fh, cx);
                                                                } else {
                                                                    let fh = this.get_or_create_button_focus_handle("security-remove-pw", cx);
                                                                    window.focus(&fh, cx);
                                                                }
                                                                cx.notify();
                                                            },
                                                        )),
                                                    ),
                                            ),
                                    )
                                }),
                        )
                    })
                    // 操作结果反馈
                    .when(result.is_some(), |d| {
                        let res = result.clone().unwrap();
                        let (msg, ok) = match &res {
                            Ok(m) => (m.clone(), true),
                            Err(m) => (m.clone(), false),
                        };
                        d.child(
                            div()
                                .px(SPACE_LG)
                                .py(SPACE_XS)
                                .text_size(ui_text_md(cx))
                                .text_color(rgb(if ok { t.success } else { t.error }))
                                .child(msg),
                        )
                    })
                    // 锁屏超时（仅增强模式）
                    .when(is_enhanced, |d| {
                        d.child(self.render_number_stepper(
                            "password-timeout",
                            &password_timeout_label,
                            s.security.password_timeout_secs as f32,
                            "{}s",
                            0.0,
                            86400.0,
                            60.0,
                            80.0,
                            false,
                            |state, val, cx| state.set_password_timeout_secs(val as u32, cx),
                            window,
                            cx,
                        ))
                    }),
            )
    }

    /// 渲染一个带可见边框、星号掩码与「显示/隐藏密码」切换按钮的密码输入框。
    fn render_password_field(&self, input: &Entity<InputState>) -> impl IntoElement {
        Input::new(input).mask_toggle(true)
    }

    /// 启用增强模式：设置主密码（Standard → Enhanced），需密码与确认密码一致。
    fn security_setup_master_password(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.security_busy = true;
        self.security_result = None;
        cx.notify();

        let pw = self.security_new_password_input.read(cx).text().to_string();
        let confirm = self
            .security_confirm_password_input
            .read(cx)
            .text()
            .to_string();
        if pw.is_empty() || confirm.is_empty() {
            self.security_busy = false;
            self.security_result = Some(Err(i18n!(cx, "settings.security.password_empty")));
            cx.notify();
            return;
        }
        if pw != confirm {
            self.security_busy = false;
            self.security_result =
                Some(Err(i18n!(cx, "settings.security.password_mismatch")));
            cx.notify();
            return;
        }

        cx.spawn(async move |this, cx| {
            let res = smol::unblock(move || {
                let mut svc = current_security_service()?;
                svc.set_master_password(&pw)?;
                Ok::<(), anyhow::Error>(())
            })
            .await;

            this.update(cx, |this, cx| {
                this.security_busy = false;
                match res {
                    Ok(()) => {
                        this.security_new_password_input
                            .update(cx, |s, cx| s.set_value("", cx));
                        this.security_confirm_password_input
                            .update(cx, |s, cx| s.set_value("", cx));
                        settings_entity(cx).update(cx, |state, cx| {
                            state.set_security_mode("enhanced".to_string(), cx);
                            state.set_master_password_set(true, cx);
                        });
                        this.security_setup_mode = false;
                        this.security_change_mode = false;
                        this.security_remove_mode = false;
                        this.security_result = None;
                    }
                    Err(e) => {
                        this.security_result = Some(Err(format!("{}", e)));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 修改主密码（Enhanced 模式）：需当前密码 + 新密码。
    fn security_change_master_password(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.security_busy = true;
        self.security_result = None;
        cx.notify();

        let current = self
            .security_current_password_input
            .read(cx)
            .text()
            .to_string();
        let new = self
            .security_new_password_input
            .read(cx)
            .text()
            .to_string();
        if current.is_empty() || new.is_empty() {
            self.security_busy = false;
            self.security_result = Some(Err(i18n!(cx, "settings.security.password_empty")));
            cx.notify();
            return;
        }

        let success_msg = i18n!(cx, "settings.security.master_password_changed");

        cx.spawn(async move |this, cx| {
            let res = smol::unblock(move || {
                let mut svc = current_security_service()?;
                svc.rotate_master_password(&current, &new)?;
                Ok::<(), anyhow::Error>(())
            })
            .await;

            this.update(cx, |this, cx| {
                this.security_busy = false;
                match res {
                    Ok(()) => {
                        this.security_current_password_input
                            .update(cx, |s, cx| s.set_value("", cx));
                        this.security_new_password_input
                            .update(cx, |s, cx| s.set_value("", cx));
                        this.security_change_mode = false;
                        this.security_remove_mode = false;
                        this.security_result = Some(Ok(success_msg));
                    }
                    Err(e) => {
                        this.security_result = Some(Err(format!("{}", e)));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 移除主密码（Enhanced → Standard）：需当前密码确认。
    fn security_remove_master_password(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.security_busy = true;
        self.security_result = None;
        cx.notify();

        let current = self
            .security_current_password_input
            .read(cx)
            .text()
            .to_string();
        if current.is_empty() {
            self.security_busy = false;
            self.security_result = Some(Err(i18n!(cx, "settings.security.password_empty")));
            cx.notify();
            return;
        }

        cx.spawn(async move |this, cx| {
            let res = smol::unblock(move || {
                let mut svc = current_security_service()?;
                svc.clear_master_password(&current)?;
                Ok::<(), anyhow::Error>(())
            })
            .await;

            this.update(cx, |this, cx| {
                this.security_busy = false;
                match res {
                    Ok(()) => {
                        this.security_current_password_input
                            .update(cx, |s, cx| s.set_value("", cx));
                        settings_entity(cx).update(cx, |state, cx| {
                            state.set_security_mode("standard".to_string(), cx);
                            state.set_master_password_set(false, cx);
                        });
                        this.security_change_mode = false;
                        this.security_remove_mode = false;
                        this.security_result = None;
                    }
                    Err(e) => {
                        this.security_result = Some(Err(format!("{}", e)));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
